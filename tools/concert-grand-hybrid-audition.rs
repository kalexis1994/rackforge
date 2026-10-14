//! Offline fixed-bridge hybrid voice, live full-mode felt attacks and repeats.
//! Soundboard observation is one-way; damping is an explicit diagonal pilot.
use super::*;
use std::io::{BufWriter, Write};

#[derive(Clone, Copy)]
struct Event {
    frame: usize,
    kind: usize,
    value: f64,
}
fn write_floats(path: &str, values: &[f64]) {
    let file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .unwrap();
    let mut w = BufWriter::new(file);
    for x in values {
        assert!(x.is_finite());
        w.write_all(&x.to_le_bytes()).unwrap();
    }
    w.flush().unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn damped_carrier_has_correct_initial_state_and_decreasing_energy() {
        let w = 1732.;
        let sigma = 8.;
        let (q, v, a) = damped(0.0002, -0.4, w * w, sigma, 0.);
        assert_eq!(q, 0.0002);
        assert_eq!(v, -0.4);
        assert_eq!(a, -w * w * q - sigma * v);
        let mut energy = 0.5 * (v * v + w * w * q * q);
        for n in 1..1000 {
            let (q, v, _) = damped(0.0002, -0.4, w * w, sigma, n as f64 * 0.00001);
            let next = 0.5 * (v * v + w * w * q * q);
            assert!(next <= energy + 1e-15);
            energy = next;
        }
        let (q, v, _) = damped(0.0002, -0.4, w * w, 0., 0.001);
        assert!((q - (0.0002 * (w * 0.001).cos() - 0.4 * (w * 0.001).sin() / w)).abs() < 1e-18);
        assert!((v - (-w * 0.0002 * (w * 0.001).sin() - 0.4 * (w * 0.001).cos())).abs() < 1e-15);
    }
    #[test]
    fn bridge_gradient_matches_independent_potential_difference() {
        let mut wire = Wire::new(
            8,
            16,
            32,
            0.62,
            0.0081,
            850.,
            200e9 * 0.0081 / 7850.,
            0.0001,
        );
        let q = (0..32)
            .map(|j| ((j * 13 + 7) as f64).sin() * 1e-6)
            .collect::<Vec<_>>();
        let g = wire.bridge_gradient(&q);
        let h = 1e-9;
        let difference =
            (wire.nonlinear_potential(&q, h) - wire.nonlinear_potential(&q, -h)) / (2. * h);
        assert!((g - difference).abs() < 1e-7 * g.abs().max(1e-12));
    }
}
// Exact underdamped linear oscillator, including diagonal viscous losses.
fn damped(q: f64, v: f64, w2: f64, sigma: f64, t: f64) -> (f64, f64, f64) {
    let a = sigma / 2.;
    let frequency = (w2 - a * a).sqrt();
    assert!(frequency.is_finite() && frequency > 0.);
    let (s, c) = (frequency * t).sin_cos();
    let decay = (-a * t).exp();
    let nq = decay * (q * c + (v + a * q) * s / frequency);
    let nv = decay * (v * c - (w2 * q + a * v) * s / frequency);
    (nq, nv, -w2 * nq - sigma * nv)
}
fn smooth(x: f64) -> f64 {
    let u = x.clamp(0., 1.);
    u * u * (3. - 2. * u)
}

pub(super) fn run(
    args: &[String],
    data: &[f64],
    nt: usize,
    nl: usize,
    full_wires: Vec<Wire>,
    initial_q: &[f64],
    initial_v: &[f64],
) {
    let low = args[3].parse::<usize>().unwrap();
    let order = args[4].parse::<usize>().unwrap();
    assert!(low > 0 && low <= nt && nl == 2 * nt);
    let p = &data[..11];
    let rate = p[4] as usize;
    let dt = 1. / rate as f64;
    let capture = rate.max(128000);
    assert!(capture % rate == 0);
    let contact_rate = rate.max(32000) * 16;
    assert!(contact_rate % capture == 0);
    let sub = contact_rate / rate;
    let stride = capture / rate;
    let total = (data[19 + 3 * nt] * rate as f64).round() as usize;
    let text = fs::read_to_string(&args[5]).unwrap();
    let events = text
        .lines()
        .map(|line| {
            let a = line
                .split_whitespace()
                .map(|s| s.parse::<f64>().unwrap())
                .collect::<Vec<_>>();
            assert!(a.len() == 3 && a.iter().all(|v| v.is_finite()));
            Event {
                frame: (a[0] * rate as f64).round() as usize,
                kind: a[1] as usize,
                value: a[2],
            }
        })
        .collect::<Vec<_>>();
    assert!(events.windows(2).all(|a| a[0].frame <= a[1].frame));
    let sigma = fs::read(&args[7])
        .unwrap()
        .chunks_exact(8)
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(sigma.len(), nt);
    assert!(sigma.iter().all(|s| s.is_finite() && *s >= 0.));
    let width = nl + 2 * nt;
    let size = 3 * width;
    let omega2 = full_wires
        .iter()
        .flat_map(|w| w.frequencies2().iter().copied())
        .collect::<Vec<_>>();
    let cross = full_wires
        .iter()
        .map(|w| w.bridge_cross().to_vec())
        .collect::<Vec<_>>();
    let indices = (0..3)
        .flat_map(|i| {
            (0..2 * low)
                .chain(nl..nl + low)
                .chain(nl + nt..nl + nt + low)
                .map(move |j| i * width + j)
        })
        .collect::<Vec<_>>();
    let area = p[1] / p[3];
    let ea = p[2] * area;
    let ei = p[2] * area * area / (4. * std::f64::consts::PI);
    let low_wires = data[11 + 3 * nt..14 + 3 * nt]
        .iter()
        .map(|t| {
            Wire::new(
                low,
                2 * low,
                (2 * low + 1).next_power_of_two(),
                p[0],
                p[1],
                *t,
                ea,
                ei,
            )
        })
        .collect();
    let mut q = initial_q.to_vec();
    let mut v = initial_v.to_vec();
    let mut model = Trajectory::new(
        low_wires,
        Vec::new(),
        indices.iter().map(|&j| q[j]).collect(),
        indices.iter().map(|&j| v[j]).collect(),
        dt,
        order,
        low,
        2 * low,
        false,
        true,
    );
    let mut contact = native::Choir::new(
        full_wires,
        1. / contact_rate as f64,
        p[5],
        p[6] / 3.,
        p[7],
        p[8],
        data[11..11 + 3 * nt].to_vec(),
        q.clone(),
        v.clone(),
    );
    let mut full_gradient = vec![0.; size];
    let mut readout_wires = data[11 + 3 * nt..14 + 3 * nt]
        .iter()
        .map(|t| {
            Wire::new(
                nt,
                nl,
                (nl.max(2 * nt) + 1).next_power_of_two(),
                p[0],
                p[1],
                *t,
                ea,
                ei,
            )
        })
        .collect::<Vec<_>>();
    let mut old_q = model.q.clone();
    let mut old_v = model.v.clone();
    let mut full_old_q = q.clone();
    let mut full_old_v = v.clone();
    let mut coast_force = vec![0.; total * stride];
    let mut segments = Vec::<(usize, usize, String)>::new();
    let mut contact_force = Vec::new();
    let mut contact_start = 0;
    let mut active = false;
    let mut contact_seen = false;
    let mut event = 0;
    let mut held = 0usize;
    let mut pedal = 0f64;
    let mut soft = 0f64;
    let mut damper_start = 1f64;
    let mut damper_target = 1f64;
    let mut damper_time = 0.;
    let mut damping = vec![0.; nt];
    let mut half = vec![0.; nt];
    let mut cached_damper = -1.;
    let mut acceleration = vec![[0.; 2]; 3 * nt * stride];
    let mut endpoint = vec![[0.; 4]; 3 * nt];
    let mut attacks = 0;
    let mut coast_steps = 0;
    let mut contact_steps = 0;
    let mut max_iterations = 0;
    let mut peak_force = 0f64;
    let mut heat = 0f64;
    let mut launches = Vec::new();
    let mut last = Instant::now();
    let start = last;
    for frame in 0..total {
        let time = frame as f64 / rate as f64;
        let damper =
            damper_start + (damper_target - damper_start) * smooth((time - damper_time) / 0.03);
        while event < events.len() && events[event].frame == frame {
            let e = events[event];
            match e.kind {
                0 => {
                    // Live strike, retaining the vibrating wires.
                    assert!(!active, "overlapping hammer contact on same key");
                    contact.set_state(&q, &v);
                    contact.set_contact_area([1. - soft, 1., 1.]);
                    let work = contact.launch(5. * (e.value / 124.).powf(1.5)).unwrap();
                    launches.push(work);
                    active = true;
                    contact_seen = false;
                    contact_start = frame * stride;
                    held += 1;
                    attacks += 1;
                }
                1 => held = held.saturating_sub(1),
                64 => pedal = e.value / 127.,
                67 => soft = e.value / 127.,
                _ => panic!("unsupported voice event"),
            }
            let target = if held > 0 { 0. } else { (1. - pedal).powf(1.5) };
            if target != damper_target {
                // Cold-prefix controls have already settled during the
                // analytically removed silence before the first strike.
                damper_start = if frame == 0 { target } else { damper };
                damper_target = target;
                damper_time = time;
            }
            event += 1;
        }
        if active {
            // The soft pedal's geometric area approximation is explicit: one
            // wire is left unstruck at full shift, preserving its live state.
            contact.set_contact_area([1. - soft, 1., 1.]);
            for _ in 0..sub {
                let (cq, cv) = contact.state();
                full_old_q.copy_from_slice(cq);
                full_old_v.copy_from_slice(cv);
                let stats = contact.step().expect("audition felt microstep");
                contact_seen |= stats.force > 0.;
                peak_force = peak_force.max(stats.force);
                heat += stats.heat;
                let (cq, cv) = contact.state();
                let mut force = 0.;
                for i in 0..3 {
                    let a = i * width;
                    let b = (i + 1) * width;
                    let g = readout_wires[i].nonlinear_avf(
                        &full_old_q[a..b],
                        &cq[a..b],
                        0.,
                        0.,
                        &mut full_gradient[a..b],
                    );
                    force -= g;
                    for n in 0..nt {
                        force -= cross[i][n]
                            * (cv[a + nl + n] - full_old_v[a + nl + n])
                            * contact_rate as f64;
                    }
                }
                // The applied felt load also acts on the endpoint lift x/L.
                // Q_bridge = x_strike*F - M_cross*q_ddot - dV_nl/db.
                force += 0.125 * stats.force;
                contact_force.push(force);
                contact_steps += 1;
            }
            let (cq, cv) = contact.state();
            q.copy_from_slice(cq);
            v.copy_from_slice(cv);
            if contact_seen && contact.separated() {
                active = false;
                for (k, &j) in indices.iter().enumerate() {
                    model.q[k] = q[j];
                    model.v[k] = v[j];
                }
                let path = format!("{}.contact-{}.f64", args[6], segments.len());
                write_floats(&path, &contact_force);
                segments.push((contact_start, contact_force.len(), path));
                contact_force.clear();
            }
        } else if attacks > 0 {
            full_old_q.copy_from_slice(&q);
            full_old_v.copy_from_slice(&v);
            old_q.copy_from_slice(&model.q);
            old_v.copy_from_slice(&model.v);
            if cached_damper != damper {
                let mass = p[1] * p[0] / 2.;
                for n in 0..nt {
                    damping[n] = sigma[n]
                        + damper * 0.6 / mass
                            * (0.5
                                - (2. * std::f64::consts::PI * (n + 1) as f64 * 0.3).cos()
                                    * (std::f64::consts::PI * (n + 1) as f64 * 0.08).sin()
                                    / (2. * std::f64::consts::PI * (n + 1) as f64 * 0.08));
                    half[n] = (-damping[n] * dt / 2.).exp();
                    for i in 0..3 {
                        let j = i * width + nl + n;
                        let (a, b, _) = damped(1., 0., omega2[j], damping[n], dt);
                        let (c, d, _) = damped(0., 1., omega2[j], damping[n], dt);
                        endpoint[i * nt + n] = [a, b, c, d];
                        for k in 0..stride {
                            let t = (k as f64 + 0.5) / stride as f64 * dt;
                            acceleration[(i * nt + n) * stride + k] = [
                                damped(1., 0., omega2[j], damping[n], t).2,
                                damped(0., 1., omega2[j], damping[n], t).2,
                            ];
                        }
                    }
                }
                cached_damper = damper;
            }
            for i in 0..3 {
                for axis in 0..2 {
                    for n in 0..low {
                        let j = i * 4 * low + 2 * low + axis * low + n;
                        model.v[j] *= half[n];
                    }
                }
            }
            let stats = model.step().expect("audition nonlinear coast");
            max_iterations = max_iterations.max(stats.iterations);
            for i in 0..3 {
                for axis in 0..2 {
                    for n in 0..low {
                        let j = i * 4 * low + 2 * low + axis * low + n;
                        model.v[j] *= half[n];
                    }
                }
            }
            let mut g0 = 0.;
            let mut g1 = 0.;
            for i in 0..3 {
                let r = i * 4 * low..(i + 1) * 4 * low;
                g0 += model.wires[i].bridge_gradient(&old_q[r.clone()]);
                g1 += model.wires[i].bridge_gradient(&model.q[r]);
            }
            for k in 0..stride {
                let theta = (k as f64 + 0.5) / stride as f64;
                let mut force = -(g0 + (g1 - g0) * theta);
                for i in 0..3 {
                    for n in 0..nt {
                        let j = i * width + nl + n;
                        let acceleration = if n < low {
                            let j = i * 4 * low + 2 * low + n;
                            ((12. * theta - 6.) * old_q[j] + (-12. * theta + 6.) * model.q[j])
                                / (dt * dt)
                                + ((6. * theta - 4.) * old_v[j] + (6. * theta - 2.) * model.v[j])
                                    / dt
                        } else if omega2[j].sqrt() < 2. * std::f64::consts::PI * 20000. {
                            let a = acceleration[(i * nt + n) * stride + k];
                            a[0] * full_old_q[j] + a[1] * full_old_v[j]
                        } else {
                            0.
                        };
                        force -= cross[i][n] * acceleration;
                    }
                }
                coast_force[frame * stride + k] = force;
            }
            for i in 0..3 {
                for n in 2 * low..nl {
                    let j = i * width + n;
                    let w = omega2[j].sqrt();
                    let (s, c) = (w * dt).sin_cos();
                    q[j] = c * full_old_q[j] + s * full_old_v[j] / w;
                    v[j] = -w * s * full_old_q[j] + c * full_old_v[j];
                }
                for axis in 0..2 {
                    for n in low..nt {
                        let j = i * width + nl + axis * nt + n;
                        let a = endpoint[i * nt + n];
                        q[j] = a[0] * full_old_q[j] + a[2] * full_old_v[j];
                        v[j] = a[1] * full_old_q[j] + a[3] * full_old_v[j];
                    }
                }
            }
            for (k, &j) in indices.iter().enumerate() {
                q[j] = model.q[k];
                v[j] = model.v[k];
            }
            // The separated head continues its free flight; parking it near
            // the moving strings would create a false contact at the repeat.
            q[size] += dt * v[size];
            coast_steps += 1;
        }
        if last.elapsed().as_secs_f64() > 20. {
            eprintln!("voice {:.2}/{:.2}s", time, total as f64 / rate as f64);
            last = Instant::now();
        }
    }
    assert!(!active, "excerpt ends during a hammer contact");
    write_floats(&args[6], &coast_force);
    let segments_json = segments
        .iter()
        .map(|(s, n, p)| {
            format!(
                "{{\"start_capture_frame\":{},\"frames\":{},\"path\":{:?}}}",
                s, n, p
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    println!("{{\"q\":{:?},\"v\":{:?},\"capture_rate_hz\":{},\"coast_rate_hz\":{},\"contact_rate_hz\":{},\"capture_frames\":{},\"attacks\":{},\"launch_work_j\":{:?},\"peak_contact_force_n\":{},\"felt_heat_j\":{},\"coast_steps\":{},\"contact_steps\":{},\"maximum_iterations\":{},\"seconds\":{},\"body_feedback\":false,\"nonlinear_transverse\":{},\"contact_segments\":[{}]}}",q,v,capture,rate,contact_rate,coast_force.len(),attacks,launches,peak_force,heat,coast_steps,contact_steps,max_iterations,start.elapsed().as_secs_f64(),low,segments_json);
}
