//! Experimental H = H_linear(all) + V_nonlinear(P_low q).
//! High coordinates are independent exact linear carriers. Only low coordinates
//! take part in the nonlinear solve and its per-wire energy projection.
use super::*;

pub(super) fn run(
    args: &[String],
    data: &[f64],
    nt: usize,
    nl: usize,
    full_wires: Vec<Wire>,
    q: &[f64],
    v: &[f64],
    frames: usize,
    order: usize,
    low: usize,
) {
    assert!(low > 0 && low <= nt && 2 * low <= nl);
    assert!(matches!(args[2].as_str(), "verify" | "bench"));
    // Other solver controls must not silently appear to affect this experiment.
    assert!(args.iter().skip(5).all(|x| x.parse::<usize>().is_ok()
        || x == "--collocation"
        || x.starts_with("--linear-highs=")
        || x.starts_with("--buffer-microframes=")
        || x.starts_with("--observation-trace=")
        || x.starts_with("--iteration-limit=")));
    let p = &data[..11];
    let width = nl + 2 * nt;
    assert!(data[14 + 3 * nt..17 + 3 * nt].iter().all(|s| *s == 0.) && v[3 * width] == 0.);
    let h = frames as f64 / p[4];
    let ln = 2 * low;
    let nodes = (ln.max(2 * low) + 1).next_power_of_two();
    let area = p[1] / p[3];
    let ea = p[2] * area;
    let ei = p[2] * area * area / (4. * std::f64::consts::PI);
    let wires = data[11 + 3 * nt..14 + 3 * nt]
        .iter()
        .map(|t| Wire::new(low, ln, nodes, p[0], p[1], *t, ea, ei))
        .collect();
    let indices = (0..3)
        .flat_map(|i| {
            (0..ln)
                .chain(nl..nl + low)
                .chain(nl + nt..nl + nt + low)
                .map(move |j| i * width + j)
        })
        .collect::<Vec<_>>();
    let omega2 = full_wires
        .iter()
        .flat_map(|w| w.frequencies2().iter().copied())
        .collect::<Vec<_>>();
    drop(full_wires);
    let mut full_q = q[..3 * width].to_vec();
    let mut full_v = v[..3 * width].to_vec();
    let mut retained = vec![false; full_q.len()];
    for &j in &indices {
        retained[j] = true;
    }
    let high = retained
        .iter()
        .enumerate()
        .filter_map(|(j, &r)| (!r).then_some(j))
        .collect::<Vec<_>>();
    drop(retained);
    let carriers = high
        .iter()
        .map(|&j| {
            let w = omega2[j].sqrt();
            ((w * h).cos(), (w * h).sin() / w)
        })
        .collect::<Vec<_>>();
    let high_energy = || {
        high.iter()
            .map(|&j| 0.5 * (v[j] * v[j] + omega2[j] * q[j] * q[j]))
            .sum::<f64>()
    };
    let high_initial = high_energy();
    let mut model = Trajectory::new(
        wires,
        Vec::new(),
        indices.iter().map(|&j| q[j]).collect(),
        indices.iter().map(|&j| v[j]).collect(),
        h,
        order,
        low,
        ln,
        false,
        true,
    );
    if let Some(limit) = args
        .iter()
        .find_map(|x| x.strip_prefix("--iteration-limit="))
    {
        model.limit = limit.parse().unwrap();
    }
    let initial_low = model.energy();
    let initial = initial_low + high_initial;
    let initial_wires = wire_energies(&mut model.wires, &model.omega2, &model.q, &model.v, None);
    let bench = args[2] == "bench";
    let buffers = if bench {
        args.get(5).map(|x| x.parse().unwrap()).unwrap_or(64)
    } else {
        0
    };
    assert!(!bench || (8..=2048).contains(&buffers));
    let buffer = args
        .iter()
        .find_map(|x| x.strip_prefix("--buffer-microframes="))
        .map(|x| x.parse::<usize>().unwrap())
        .unwrap_or(256);
    assert!(buffer >= frames && buffer <= 4096 && buffer % frames == 0);
    let total = if bench {
        buffer * buffers
    } else {
        (data[19 + 3 * nt] * p[4]).round() as usize
    };
    assert!(total > 0 && total % frames == 0);
    // Low-only trace: the driver adds the exact high linear signal on a dense
    // grid BEFORE audio-rate anti-alias filtering. Sampling highs here aliases.
    let mut observation = args
        .iter()
        .find_map(|x| x.strip_prefix("--observation-trace="))
        .map(|path| {
            assert!(!bench);
            observation::Observation::new(path.to_string(), low, ln, p[0], p[1], total / frames)
        });
    if let Some(o) = &mut observation {
        o.sample(0., &model.v);
    }
    let mut times = Vec::with_capacity(buffers);
    let (mut completed, mut iterations, mut work, mut residual) = (0, 0, 0f64, 0f64);
    let mut failure = None;
    let before = phase::allocation_count();
    let start = Instant::now();
    let mut last = start;
    while completed < total {
        // If low solve fails, neither high carriers nor public state advance.
        match black_box(model.step()) {
            Ok(stats) => {
                iterations = iterations.max(stats.iterations);
                residual = residual.max(stats.residual);
                for (&j, &(c, s)) in high.iter().zip(&carriers) {
                    let old_q = full_q[j];
                    full_q[j] = c * old_q + s * full_v[j];
                    full_v[j] = -omega2[j] * s * old_q + c * full_v[j];
                }
                for (k, &j) in indices.iter().enumerate() {
                    full_q[j] = model.q[k];
                    full_v[j] = model.v[k];
                }
                completed += frames;
                if let Some(o) = &mut observation {
                    o.sample(completed as f64 / p[4], &model.v);
                }
                let he = high
                    .iter()
                    .map(|&j| 0.5 * (full_v[j] * full_v[j] + omega2[j] * full_q[j] * full_q[j]))
                    .sum::<f64>();
                work = work.max((model.energy() + he - initial).abs() / initial.abs().max(1e-30));
                if bench && completed % buffer == 0 {
                    times.push(last.elapsed().as_secs_f64() * 1000.);
                    last = Instant::now();
                }
            }
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }
    let seconds = start.elapsed().as_secs_f64();
    let allocations = phase::allocation_count() - before;
    let final_wires = wire_energies(&mut model.wires, &model.omega2, &model.q, &model.v, None);
    if let Some(o) = &observation {
        o.write();
    }
    let mean = times.iter().sum::<f64>() / times.len().max(1) as f64;
    times.sort_by(f64::total_cmp);
    let maximum = times.last().copied().unwrap_or(0.);
    let prepared = model.prepare_bytes()
        + (full_q.capacity() + full_v.capacity() + omega2.capacity()) * 8
        + (indices.capacity() + high.capacity()) * std::mem::size_of::<usize>()
        + carriers.capacity() * 16;
    full_q.push(q[3 * width]);
    full_v.push(0.);
    let fail = failure
        .map(|e| format!("{:?}", e))
        .unwrap_or_else(|| "null".into());
    println!("{{\"q\":{:?},\"v\":{:?},\"completed\":{},\"completed_microframes\":{},\"failure\":{},\"linear_transverse_per_wire\":{},\"nonlinear_transverse_per_wire\":{},\"nonlinear_axial_per_wire\":{},\"initial_energy_j\":{},\"initial_low_wire_energy_j\":{:?},\"final_low_wire_energy_j\":{:?},\"work_relative_max\":{},\"maximum_iterations\":{},\"midpoint_residual_relative_max\":{},\"allocations\":{},\"seconds\":{},\"buffers\":{},\"mean_ms\":{},\"max_ms\":{},\"known_prepared_bytes\":{},\"trace_low_only\":true,\"audio_output_benchmarked\":false,\"full_piano_realtime\":false}}",full_q,full_v,failure.is_none(),completed,fail,nt,low,ln,initial,initial_wires,final_wires,work,iterations,residual,allocations,seconds,times.len(),mean,maximum,prepared);
}
