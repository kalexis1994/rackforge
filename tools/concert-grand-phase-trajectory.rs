//! Experimental fixed-bridge string trajectory, without hammer/contact.
//! Averaged interaction midpoint with an explicit numerical energy constraint.
//! Temporal accuracy must be checked independently; closed energy is not proof.
#[path = "concert-grand-collocation.rs"]
mod collocation;
#[path = "concert-grand-phase-profile.rs"]
mod profile;
#[path = "concert-grand-observation.rs"]
mod observation;
#[path = "concert-grand-hybrid.rs"]
mod hybrid;
#[path = "concert-grand-hybrid-audition.rs"]
mod audition;
#[allow(dead_code)]
#[path = "concert-grand-native-strings.rs"]
mod native;
#[allow(dead_code)]
#[path = "concert-grand-phase-forces.rs"]
mod phase;
use native::Wire;
use phase::{AxialWaveform, PhaseQuadratic};
use std::{env, fs, hint::black_box, time::Instant};

fn gauss(order: usize, h: f64) -> Vec<(f64, f64)> {
    assert!((8..=128).contains(&order));
    let mut rule = vec![(0., 0.); order];
    for i in 0..order.div_ceil(2) {
        let mut x = (std::f64::consts::PI * (i as f64 + 0.75) / (order as f64 + 0.5)).cos();
        let mut derivative = 0.;
        for _ in 0..50 {
            let (mut a, mut b) = (1., x);
            for k in 2..=order {
                let c = ((2 * k - 1) as f64 * x * b - (k - 1) as f64 * a) / k as f64;
                a = b;
                b = c;
            }
            derivative = order as f64 * (x * b - a) / (x * x - 1.);
            let next = x - b / derivative;
            if (next - x).abs() <= 4. * f64::EPSILON {
                x = next;
                break;
            }
            x = next;
        }
        let weight = h / ((1. - x * x) * derivative * derivative);
        rule[i] = ((1. - x) * h / 2., weight);
        rule[order - 1 - i] = ((1. + x) * h / 2., weight);
    }
    rule
}

fn energy(
    wires: &mut [Wire],
    omega2: &[f64],
    q: &[f64],
    v: &[f64],
    gradient: Option<&mut [f64]>,
) -> f64 {
    let width = wires[0].size();
    let mut value = 0.5
        * q.iter()
            .zip(v)
            .zip(omega2)
            .map(|((q, v), w)| v * v + w * q * q)
            .sum::<f64>();
    if let Some(gradient) = gradient {
        for (i, wire) in wires.iter_mut().enumerate() {
            let range = i * width..(i + 1) * width;
            value += wire.point_nonlinearity(&q[range.clone()], &mut gradient[range]);
        }
    } else {
        for (i, wire) in wires.iter_mut().enumerate() {
            value += wire.nonlinear_potential(&q[i * width..(i + 1) * width], 0.);
        }
    }
    value
}

fn wire_energies(
    wires: &mut [Wire],
    omega2: &[f64],
    q: &[f64],
    v: &[f64],
    gradient: Option<&mut [f64]>,
) -> [f64; 3] {
    let width = wires[0].size();
    let mut values = [0.; 3];
    let mut gradient = gradient;
    for (i, wire) in wires.iter_mut().enumerate() {
        let range = i * width..(i + 1) * width;
        values[i] = 0.5
            * (range.clone())
                .map(|j| v[j] * v[j] + omega2[j] * q[j] * q[j])
                .sum::<f64>();
        values[i] += if let Some(ref mut g) = gradient {
            wire.point_nonlinearity(&q[range.clone()], &mut g[range])
        } else {
            wire.nonlinear_potential(&q[range], 0.)
        };
    }
    values
}

#[derive(Default)]
struct Stats {
    iterations: usize,
    lambda: f64,
    raw_work_relative: f64,
    residual: f64,
}
struct Trajectory {
    profile: profile::Profile,
    collocation: Option<collocation::Collocation>,
    wires: Vec<Wire>,
    plans: Vec<PhaseQuadratic>,
    waves: Vec<AxialWaveform>,
    forced_axial: bool,
    transverse_sample_gradient: bool,
    omega2: Vec<f64>,
    q: Vec<f64>,
    v: Vec<f64>,
    c: Vec<f64>,
    s: Vec<f64>,
    sample_transfer: Vec<[f64; 4]>,
    transverse_indices: Vec<usize>,
    order: usize,
    center_q: Vec<f64>,
    center_v: Vec<f64>,
    kernel_q: Vec<f64>,
    kernel_v: Vec<f64>,
    free_q: Vec<f64>,
    free_v: Vec<f64>,
    delta_q: Vec<f64>,
    delta_v: Vec<f64>,
    raw_q: Vec<f64>,
    raw_v: Vec<f64>,
    end_q: Vec<f64>,
    end_v: Vec<f64>,
    direction_q: Vec<f64>,
    direction_v: Vec<f64>,
    sample_q: Vec<f64>,
    gradient: Vec<f64>,
    constrained: bool,
    per_wire_constraint: bool,
    limit: usize,
}
impl Trajectory {
    fn new(
        mut wires: Vec<Wire>,
        plans: Vec<PhaseQuadratic>,
        q: Vec<f64>,
        v: Vec<f64>,
        h: f64,
        order: usize,
        nt: usize,
        nl: usize,
        forced_axial: bool,
        use_collocation: bool,
    ) -> Self {
        let omega2 = wires
            .iter()
            .flat_map(|wire| wire.frequencies2().iter().copied())
            .collect::<Vec<_>>();
        let size = omega2.len();
        let width = nl + 2 * nt;
        assert!(q.len() == size && v.len() == size);
        let c = omega2
            .iter()
            .map(|w| (w.sqrt() * h).cos())
            .collect::<Vec<_>>();
        let s = omega2
            .iter()
            .map(|w| (w.sqrt() * h).sin() / w.sqrt())
            .collect::<Vec<_>>();
        let transverse_indices = if use_collocation {
            Vec::new()
        } else {
            (0..3)
                .flat_map(|i| i * width + nl..(i + 1) * width)
                .collect::<Vec<_>>()
        };
        let mut sample_transfer = Vec::with_capacity(order * transverse_indices.len());
        let rule = if use_collocation {
            for wire in &mut wires {
                wire.prepare_batch4();
                wire.prepare_axial_elimination(order);
            }
            Vec::new()
        } else {
            gauss(order, h)
        };
        let waves = if forced_axial {
            plans
                .iter()
                .map(|p| p.axial_waveform(&rule.iter().map(|(t, _)| *t).collect::<Vec<_>>()))
                .collect()
        } else {
            Vec::new()
        };
        for (t, weight) in rule {
            for &j in &transverse_indices {
                let w = omega2[j].sqrt();
                let (st, ct) = (w * t).sin_cos();
                let (sr, cr) = (w * (h - t)).sin_cos();
                sample_transfer.push([ct, st / w, -weight * sr / w, -weight * cr]);
            }
        }
        Self {
            profile: profile::Profile::default(),
            collocation: use_collocation.then(|| collocation::Collocation::new(&omega2, h, order)),
            wires,
            plans,
            waves,
            forced_axial,
            transverse_sample_gradient: true,
            omega2,
            q,
            v,
            c,
            s,
            sample_transfer,
            transverse_indices,
            order,
            center_q: vec![0.; size],
            center_v: vec![0.; size],
            kernel_q: vec![0.; size],
            kernel_v: vec![0.; size],
            free_q: vec![0.; size],
            free_v: vec![0.; size],
            delta_q: vec![0.; size],
            delta_v: vec![0.; size],
            raw_q: vec![0.; size],
            raw_v: vec![0.; size],
            end_q: vec![0.; size],
            end_v: vec![0.; size],
            direction_q: vec![0.; size],
            direction_v: vec![0.; size],
            sample_q: vec![0.; size],
            gradient: vec![0.; size],
            constrained: true,
            per_wire_constraint: true,
            limit: 32,
        }
    }
    fn energy(&mut self) -> f64 {
        energy(&mut self.wires, &self.omega2, &self.q, &self.v, None)
    }
    fn prepare_bytes(&self) -> usize {
        self.plans.iter().map(|p| p.prepared_bytes()).sum::<usize>()
            + self.sample_transfer.capacity() * std::mem::size_of::<[f64; 4]>()
            + self.waves.iter().map(|w| w.prepared_bytes()).sum::<usize>()
            + self
                .collocation
                .as_ref()
                .map(|c| c.prepared_bytes())
                .unwrap_or(0)
            + self
                .wires
                .iter()
                .map(|w| w.batch_prepared_bytes())
                .sum::<usize>()
    }
    fn project(&mut self, target: f64, mut lambda: f64) -> Result<(f64, f64), &'static str> {
        for _ in 0..12 {
            for j in 0..self.q.len() {
                self.end_q[j] = self.raw_q[j] - lambda * self.direction_q[j];
                self.end_v[j] = self.raw_v[j] - lambda * self.direction_v[j];
            }
            let value = energy(
                &mut self.wires,
                &self.omega2,
                &self.end_q,
                &self.end_v,
                Some(&mut self.gradient),
            );
            if !value.is_finite() {
                return Err("nonfinite energy root");
            }
            if (value - target).abs() <= 1e-13 * target.abs().max(1e-30) {
                return Ok((lambda, value));
            }
            let derivative = -(0..self.q.len())
                .map(|j| {
                    (self.omega2[j] * self.end_q[j] + self.gradient[j]) * self.direction_q[j]
                        + self.end_v[j] * self.direction_v[j]
                })
                .sum::<f64>();
            if !derivative.is_finite() || derivative.abs() <= 1e-30 {
                return Err("singular energy root");
            }
            lambda -= (value - target) / derivative;
            // Numerical root basin, not a material/amplitude cutoff.
            if !lambda.is_finite() || lambda.abs() > 0.25 {
                return Err("energy root left local basin");
            }
        }
        Err("energy root did not converge")
    }
    fn project_wires(
        &mut self,
        target: &[f64; 3],
        mut lambda: [f64; 3],
    ) -> Result<[f64; 3], &'static str> {
        let width = self.wires[0].size();
        for _ in 0..12 {
            for j in 0..self.q.len() {
                self.end_q[j] = self.raw_q[j] - lambda[j / width] * self.direction_q[j];
                self.end_v[j] = self.raw_v[j] - lambda[j / width] * self.direction_v[j];
            }
            let values = wire_energies(
                &mut self.wires,
                &self.omega2,
                &self.end_q,
                &self.end_v,
                Some(&mut self.gradient),
            );
            let mut converged = true;
            for i in 0..3 {
                if !values[i].is_finite() {
                    return Err("nonfinite wire energy root");
                }
                if (values[i] - target[i]).abs() <= 1e-13 * target[i].abs().max(1e-30) {
                    continue;
                }
                converged = false;
                let derivative = -(i * width..(i + 1) * width)
                    .map(|j| {
                        (self.omega2[j] * self.end_q[j] + self.gradient[j]) * self.direction_q[j]
                            + self.end_v[j] * self.direction_v[j]
                    })
                    .sum::<f64>();
                if !derivative.is_finite() || derivative.abs() <= 1e-30 {
                    return Err("singular wire energy root");
                }
                lambda[i] -= (values[i] - target[i]) / derivative;
                if !lambda[i].is_finite() || lambda[i].abs() > 0.25 {
                    return Err("wire energy root left local basin");
                }
            }
            if converged {
                return Ok(lambda);
            }
        }
        Err("wire energy roots did not converge")
    }
    fn step(&mut self) -> Result<Stats, &'static str> {
        if self.collocation.is_some() {
            return self.step_collocation();
        }
        let size = self.q.len();
        let width = self.wires[0].size();
        let start_energy = self.energy();
        let target_wires = if self.per_wire_constraint {
            wire_energies(&mut self.wires, &self.omega2, &self.q, &self.v, None)
        } else {
            [0.; 3]
        };
        for j in 0..size {
            self.center_q[j] = self.q[j];
            self.center_v[j] = self.v[j];
            self.free_q[j] = self.c[j] * self.q[j] + self.s[j] * self.v[j];
            self.free_v[j] = -self.omega2[j] * self.s[j] * self.q[j] + self.c[j] * self.v[j];
        }
        energy(
            &mut self.wires,
            &self.omega2,
            &self.free_q,
            &self.free_v,
            Some(&mut self.gradient),
        );
        for j in 0..size {
            // Canonical direction fixed from the freely transported start.
            // The scalar root is coupled into the midpoint iteration below.
            self.direction_q[j] = self.free_q[j] + self.gradient[j] / self.omega2[j];
            self.direction_v[j] = self.free_v[j];
        }
        let scale = (0..size)
            .map(|j| self.omega2[j] * self.q[j] * self.q[j] + self.v[j] * self.v[j])
            .sum::<f64>()
            .sqrt()
            .max(1e-30);
        let mut lambda = 0.;
        let mut wire_lambda = [0.; 3];
        let nl = width - self.transverse_indices.len() / 3;
        for iteration in 1..=self.limit {
            self.kernel_q.copy_from_slice(&self.center_q);
            self.kernel_v.copy_from_slice(&self.center_v);
            if self.forced_axial {
                for (i, wave) in self.waves.iter_mut().enumerate() {
                    let range = i * width..(i + 1) * width;
                    wave.set_carriers(&self.center_q[range.clone()], &self.center_v[range]);
                    wave.initial_particular(
                        &mut self.sample_q[i * width..i * width + nl],
                        &mut self.gradient[i * width..i * width + nl],
                    );
                    for j in i * width..i * width + nl {
                        self.kernel_q[j] -= self.sample_q[j];
                        self.kernel_v[j] -= self.gradient[j];
                    }
                }
            }
            for (i, plan) in self.plans.iter_mut().enumerate() {
                let range = i * width..(i + 1) * width;
                plan.apply(
                    &self.kernel_q[range.clone()],
                    &self.kernel_v[range.clone()],
                    &mut self.delta_q[range.clone()],
                    &mut self.delta_v[range],
                );
            }
            let ntotal = self.transverse_indices.len();
            for sample in 0..self.order {
                if self.forced_axial {
                    for (i, wave) in self.waves.iter().enumerate() {
                        wave.sample(sample, &mut self.sample_q[i * width..i * width + nl]);
                    }
                }
                for (k, &j) in self.transverse_indices.iter().enumerate() {
                    let t = self.sample_transfer[sample * ntotal + k];
                    self.sample_q[j] = t[0] * self.center_q[j] + t[1] * self.center_v[j];
                }
                for (i, wire) in self.wires.iter_mut().enumerate() {
                    let range = i * width..(i + 1) * width;
                    if self.forced_axial {
                        // gu is already integrated analytically; add only the
                        // transverse gradient of forced u and the quartic term.
                        if self.transverse_sample_gradient {
                            wire.point_transverse_nonlinearity(
                                &self.sample_q[range.clone()],
                                &mut self.gradient[range],
                            );
                        } else {
                            wire.point_nonlinearity(
                                &self.sample_q[range.clone()],
                                &mut self.gradient[range],
                            );
                        }
                    } else {
                        wire.point_cubic(&self.sample_q[range.clone()], &mut self.gradient[range]);
                    }
                }
                for (k, &j) in self.transverse_indices.iter().enumerate() {
                    let t = self.sample_transfer[sample * ntotal + k];
                    self.delta_q[j] += t[2] * self.gradient[j];
                    self.delta_v[j] += t[3] * self.gradient[j];
                }
            }
            for j in 0..size {
                self.raw_q[j] = self.free_q[j] + self.delta_q[j];
                self.raw_v[j] = self.free_v[j] + self.delta_v[j];
            }
            let raw_energy = energy(
                &mut self.wires,
                &self.omega2,
                &self.raw_q,
                &self.raw_v,
                None,
            );
            let raw_work = (raw_energy - start_energy).abs() / start_energy.abs().max(1e-30);
            if self.constrained {
                if self.per_wire_constraint {
                    wire_lambda = self.project_wires(&target_wires, wire_lambda)?;
                    lambda = wire_lambda.iter().map(|x| x.abs()).fold(0., f64::max);
                } else {
                    lambda = self.project(start_energy, lambda)?.0;
                }
            } else {
                self.end_q.copy_from_slice(&self.raw_q);
                self.end_v.copy_from_slice(&self.raw_v);
            }
            let mut residual2 = 0.;
            for j in 0..size {
                let forced = self.forced_axial && j % width < nl;
                let endq = self.end_q[j] - if forced { self.delta_q[j] } else { 0. };
                let endv = self.end_v[j] - if forced { self.delta_v[j] } else { 0. };
                let cq = 0.5 * (self.q[j] + self.c[j] * endq - self.s[j] * endv);
                let cv = 0.5 * (self.v[j] + self.omega2[j] * self.s[j] * endq + self.c[j] * endv);
                residual2 += self.omega2[j] * (cq - self.center_q[j]).powi(2)
                    + (cv - self.center_v[j]).powi(2);
                self.center_q[j] = cq;
                self.center_v[j] = cv;
            }
            let residual = residual2.sqrt() / scale;
            if !residual.is_finite() {
                return Err("nonfinite midpoint residual");
            }
            if residual <= 1e-11 {
                self.q.copy_from_slice(&self.end_q);
                self.v.copy_from_slice(&self.end_v);
                return Ok(Stats {
                    iterations: iteration,
                    lambda,
                    raw_work_relative: raw_work,
                    residual,
                });
            }
        }
        // No physical q/v is published before both conditions converge.
        Err("midpoint iteration did not converge")
    }
    fn step_collocation(&mut self) -> Result<Stats, &'static str> {
        let clock = profile::start();
        let target = self.energy();
        let target_wires = wire_energies(&mut self.wires, &self.omega2, &self.q, &self.v, None);
        for j in 0..self.q.len() {
            self.free_q[j] = self.c[j] * self.q[j] + self.s[j] * self.v[j];
            self.free_v[j] = -self.omega2[j] * self.s[j] * self.q[j] + self.c[j] * self.v[j];
        }
        energy(
            &mut self.wires,
            &self.omega2,
            &self.free_q,
            &self.free_v,
            Some(&mut self.gradient),
        );
        for j in 0..self.q.len() {
            self.direction_q[j] = self.free_q[j] + self.gradient[j] / self.omega2[j];
            self.direction_v[j] = self.free_v[j];
        }
        self.profile.record(profile::Stage::StepSetup, clock);
        let (iterations, residual) = self.collocation.as_mut().unwrap().solve(
            &mut self.wires,
            &self.omega2,
            &self.q,
            &self.v,
            &mut self.delta_q,
            &mut self.delta_v,
            self.limit,
            &mut self.profile,
        )?;
        let clock = profile::start();
        for j in 0..self.q.len() {
            self.raw_q[j] = self.free_q[j] + self.delta_q[j];
            self.raw_v[j] = self.free_v[j] + self.delta_v[j];
        }
        let raw = energy(
            &mut self.wires,
            &self.omega2,
            &self.raw_q,
            &self.raw_v,
            None,
        );
        self.profile.record(profile::Stage::RawEnergy, clock);
        let clock = profile::start();
        let lambda = if !self.constrained {
            self.end_q.copy_from_slice(&self.raw_q);
            self.end_v.copy_from_slice(&self.raw_v);
            0.
        } else if self.per_wire_constraint {
            self.project_wires(&target_wires, [0.; 3])?
                .iter()
                .map(|x| x.abs())
                .fold(0., f64::max)
        } else {
            self.project(target, 0.)?.0.abs()
        };
        self.profile.record(profile::Stage::Projection, clock);
        let clock = profile::start();
        self.q.copy_from_slice(&self.end_q);
        self.v.copy_from_slice(&self.end_v);
        self.profile.record(profile::Stage::Publish, clock);
        Ok(Stats {
            iterations,
            lambda,
            residual,
            raw_work_relative: (raw - target).abs() / target.abs().max(1e-30),
        })
    }
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    let bytes = fs::read(&args[1]).unwrap();
    assert!(bytes.len() >= 20 && &bytes[..8] == b"RFSTR001");
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let (nt, nl, nodes) = (u32_at(8), u32_at(12), u32_at(16));
    let width = nl + 2 * nt;
    let size = 3 * width + 1;
    assert!(
        nt > 0
            && nt <= 256
            && nl > 0
            && nl <= 512
            && bytes.len() == 20 + 8 * (11 + 3 * nt + 9 + 3 * size)
    );
    let data = bytes[20..]
        .chunks_exact(8)
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert!(data.iter().all(|v| v.is_finite()));
    let p = &data[..11];
    let tensions = &data[11 + 3 * nt..14 + 3 * nt];
    let state_start = 20 + 3 * nt;
    let q = &data[state_start..state_start + size];
    let v = &data[state_start + 2 * size..state_start + 3 * size];
    let area = p[1] / p[3];
    let ea = p[2] * area;
    let ei = p[2] * area * area / (4. * std::f64::consts::PI);
    let mut wires = tensions
        .iter()
        .map(|t| Wire::new(nt, nl, nodes, p[0], p[1], *t, ea, ei))
        .collect::<Vec<_>>();
    if args.get(2).is_some_and(|x| x == "midi-voice") {
        audition::run(&args, &data, nt, nl, wires, q, v);
        return;
    }
    if args.get(2).is_some_and(|x| x == "batch-gradient") {
        let stride = 3 * width;
        let mut inputs = vec![0.; 4 * stride];
        let mut batch = inputs.clone();
        let mut single = inputs.clone();
        for (lane, factor) in [1., -0.25, 0., 1e-12].iter().enumerate() {
            for j in 0..stride {
                inputs[lane * stride + j] = factor * q[j];
            }
        }
        for (i, wire) in wires.iter_mut().enumerate() {
            wire.prepare_batch4();
            if args.iter().any(|x| x == "--legacy-batch-packing") {
                wire.legacy_batch_packing();
            }
            if args.iter().any(|x| x == "--split-batch-forces") {
                wire.prepare_axial_elimination(4);
                wire.axial_force_batch4(&inputs[i * width..], stride, &mut batch[i * width..], 0);
                wire.transverse_force_batch4(&inputs[i * width..], stride, &mut batch[i * width..], 0);
            } else {
                wire.point_nonlinearity_batch4(&inputs[i * width..], stride, &mut batch[i * width..]);
            }
            for lane in 0..4 {
                let range = lane * stride + i * width..lane * stride + (i + 1) * width;
                wire.point_nonlinearity(&inputs[range.clone()], &mut single[range]);
            }
        }
        println!(
            "{{\"gradient\":{:?},\"single_gradient\":{:?},\"q_nodes\":{:?}}}",
            batch, single, inputs
        );
        return;
    }
    if args.get(2).is_some_and(|x| x == "transverse-gradient") {
        let mut full = vec![0.; 3 * width];
        let mut partial = full.clone();
        let mut full_after = full.clone();
        for (i, wire) in wires.iter_mut().enumerate() {
            let range = i * width..(i + 1) * width;
            wire.point_nonlinearity(&q[range.clone()], &mut full[range.clone()]);
            wire.point_transverse_nonlinearity(&q[range.clone()], &mut partial[range.clone()]);
            wire.point_nonlinearity(&q[range.clone()], &mut full_after[range]);
        }
        println!(
            "{{\"gradient\":{:?},\"full_gradient\":{:?},\"full_gradient_after\":{:?}}}",
            partial, full, full_after
        );
        return;
    }
    if args.get(2).is_some_and(|x| x == "cubic") {
        let mut gradient = vec![0.; 3 * width];
        for (i, wire) in wires.iter_mut().enumerate() {
            wire.point_cubic(
                &q[i * width..(i + 1) * width],
                &mut gradient[i * width..(i + 1) * width],
            );
        }
        println!("{{\"gradient\":{:?}}}", gradient);
        return;
    }
    let frames = args
        .get(3)
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(16);
    let order = args
        .get(4)
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(64);
    assert!((1..=64).contains(&frames) && 256 % frames == 0 && p[4] > 0.);
    let h = frames as f64 / p[4];
    if let Some(low) = args.iter().find_map(|x| x.strip_prefix("--linear-highs=")) {
        assert!(args.iter().any(|x| x == "--collocation"));
        hybrid::run(&args, &data, nt, nl, wires, q, v, frames, order, low.parse().unwrap());
        return;
    }
    if args.get(2).is_some_and(|x| x == "collocation-rule") {
        let rule = gauss(order, h);
        let serial_rule = rule.iter().map(|&(t, w)| [t, w]).collect::<Vec<_>>();
        println!(
            "{{\"rule\":{:?},\"matrix\":{:?},\"h\":{}}}",
            serial_rule,
            collocation::integration_matrix(&rule, h),
            h
        );
        return;
    }
    let root = (p[1] * p[0] / 2.).sqrt();
    let ku = (1..=nl)
        .map(|n| std::f64::consts::PI * n as f64 / p[0] / root)
        .collect::<Vec<_>>();
    let kv = (1..=nt)
        .map(|n| std::f64::consts::PI * n as f64 / p[0] / root)
        .collect::<Vec<_>>();
    let use_collocation = args.iter().any(|x| x == "--collocation");
    let plans = if use_collocation {
        Vec::new()
    } else {
        wires
            .iter()
            .map(|w| {
                PhaseQuadratic::new(
                    nt,
                    nl,
                    w.frequencies2().iter().map(|w| w.sqrt()).collect(),
                    &ku,
                    &kv,
                    ea * p[0],
                    h,
                )
            })
            .collect::<Vec<_>>()
    };
    if args.get(2).is_some_and(|x| x == "waveform") {
        let times = [0., 0.17 * h, 0.5 * h, 0.83 * h, h];
        let mut samples = vec![vec![0.; 3 * nl]; times.len()];
        let mut high = Vec::new();
        let mut axial_omega = Vec::new();
        for (i, plan) in plans.iter().enumerate() {
            let mut wave = plan.axial_waveform(&times);
            if args
                .iter()
                .any(|x| x == "--portable" || x == "--scalar-axial-waveform")
            {
                wave.set_portable();
            }
            wave.set_carriers(
                &q[i * width..(i + 1) * width],
                &v[i * width..(i + 1) * width],
            );
            let mut shiftq = vec![0.; nl];
            let mut shiftv = vec![0.; nl];
            wave.initial_particular(&mut shiftq, &mut shiftv);
            high.push(wave.high_modes());
            for &w in &wires[i].frequencies2()[..nl] {
                axial_omega.push(w.sqrt());
            }
            for (k, &t) in times.iter().enumerate() {
                wave.sample(k, &mut samples[k][i * nl..(i + 1) * nl]);
                for j in 0..nl {
                    let w = wires[i].frequencies2()[j].sqrt();
                    samples[k][i * nl + j] -=
                        shiftq[j] * (w * t).cos() + shiftv[j] * (w * t).sin() / w;
                }
            }
        }
        println!("{{\"times_s\":{:?},\"driven_axial_q\":{:?},\"omega_axial\":{:?},\"high_modes_per_wire\":{:?}}}",times,samples,axial_omega,high);
        return;
    }
    // Explicit string-only coast experiment; the hammer is not simulated.
    assert!(data[14 + 3 * nt..17 + 3 * nt].iter().all(|s| *s == 0.) && v[size - 1] == 0.);
    let mut model = Trajectory::new(
        wires,
        plans,
        q[..3 * width].to_vec(),
        v[..3 * width].to_vec(),
        h,
        order,
        nt,
        nl,
        !use_collocation && !args.iter().any(|x| x == "--no-axial-waveform"),
        use_collocation,
    );
    model.constrained = !args.iter().any(|x| x == "--unconstrained");
    if args.iter().any(|x| x == "--audit-full-collocation-residual") {
        if let Some(collocation) = &mut model.collocation { collocation.enable_residual_audit(); }
    }
    if args.iter().any(|x| x == "--simultaneous-collocation") {
        if let Some(collocation) = &mut model.collocation { collocation.simultaneous_forces(); }
        for wire in &mut model.wires { wire.disable_axial_elimination_cache(); }
    }
    if args.iter().any(|x| x == "--streamed-node-contraction") {
        if let Some(collocation) = &mut model.collocation {
            collocation.streamed_node_contraction();
        }
    }
    if args.iter().any(|x| x == "--legacy-batch-packing") {
        for wire in &mut model.wires {
            wire.legacy_batch_packing();
        }
    }
    if args.iter().any(|x| x == "--legacy-node-updates") {
        if let Some(collocation) = &mut model.collocation {
            collocation.legacy_node_updates();
        }
    }
    if args.iter().any(|x| x == "--scalar-collocation-forces") {
        if let Some(collocation) = &mut model.collocation {
            collocation.scalar_forces();
        }
        for wire in &mut model.wires {
            wire.disable_batch4();
        }
    }
    if args.iter().any(|x| x == "--refresh-converged-forces") {
        if let Some(collocation) = &mut model.collocation {
            collocation.refresh_converged_forces();
        }
    }
    model.transverse_sample_gradient = !args.iter().any(|x| x == "--full-sample-gradient");
    model.per_wire_constraint = !args.iter().any(|x| x == "--shared-energy-root");
    if let Some(limit) = args
        .iter()
        .find_map(|x| x.strip_prefix("--iteration-limit="))
    {
        model.limit = limit.parse().unwrap();
    }
    if args.iter().any(|x| x == "--portable") {
        for plan in &mut model.plans {
            plan.set_portable();
        }
    }
    if args
        .iter()
        .any(|x| x == "--portable" || x == "--scalar-axial-waveform")
    {
        for wave in &mut model.waves {
            wave.set_portable();
        }
    }
    let initial = model.energy();
    let initial_wires = wire_energies(&mut model.wires, &model.omega2, &model.q, &model.v, None);
    let start = Instant::now();
    let bench = args.get(2).is_some_and(|x| x == "bench");
    let buffers = if bench {
        args.get(5)
            .map(|s| s.parse::<usize>().unwrap())
            .unwrap_or(64)
    } else {
        0
    };
    assert!(!bench || (8..=2048).contains(&buffers));
    let buffer_microframes = args
        .iter()
        .find_map(|x| x.strip_prefix("--buffer-microframes="))
        .map(|x| x.parse::<usize>().unwrap())
        .unwrap_or(256);
    assert!(
        buffer_microframes >= frames
            && buffer_microframes <= 4096
            && buffer_microframes % frames == 0
    );
    let total = if bench {
        buffer_microframes * buffers
    } else {
        (data[19 + 3 * nt] * p[4]).round() as usize
    };
    assert!(total % frames == 0 && total > 0);
    let mut observation = args.iter().find_map(|x| x.strip_prefix("--observation-trace="))
        .map(|path| {
            assert!(!bench, "observation traces are excluded from CPU benchmarks");
            observation::Observation::new(path.to_string(), nt, nl, p[0], p[1], total / frames)
        });
    if let Some(o) = &mut observation { o.sample(0., &model.v); }
    let mut times = Vec::with_capacity(buffers);
    let mut last = Instant::now();
    let allocations_before = phase::allocation_count();
    let (
        mut completed,
        mut max_iterations,
        mut lambda_max,
        mut raw_max,
        mut residual_max,
        mut work_max,
    ) = (0, 0, 0f64, 0f64, 0f64, 0f64);
    let mut failure = None;
    while completed < total {
        match black_box(model.step()) {
            Ok(stats) => {
                max_iterations = max_iterations.max(stats.iterations);
                lambda_max = lambda_max.max(stats.lambda.abs());
                raw_max = raw_max.max(stats.raw_work_relative);
                residual_max = residual_max.max(stats.residual);
                completed += frames;
                if let Some(o) = &mut observation { o.sample(completed as f64 / p[4], &model.v); }
                let clock = profile::start();
                work_max =
                    work_max.max((model.energy() - initial).abs() / initial.abs().max(1e-30));
                model.profile.record(profile::Stage::Ledger, clock);
                if bench && completed % buffer_microframes == 0 {
                    times.push(last.elapsed().as_secs_f64() * 1000.);
                    last = Instant::now();
                }
            }
            Err(error) => {
                failure = Some(error);
                break;
            }
        }
    }
    let allocations = phase::allocation_count() - allocations_before;
    let elapsed = start.elapsed().as_secs_f64();
    let observation_samples = observation.as_ref().map(|o|o.len()).unwrap_or(0);
    if let Some(o) = &observation { o.write(); }
    let energy = model.energy();
    let final_wires = wire_energies(&mut model.wires, &model.omega2, &model.q, &model.v, None);
    model.q.push(q[size - 1]);
    model.v.push(0.);
    let mean = if times.is_empty() {
        0.
    } else {
        times.iter().sum::<f64>() / times.len() as f64
    };
    times.sort_by(f64::total_cmp);
    let p99 = times.get(times.len() * 99 / 100).copied().unwrap_or(0.);
    let maximum = times.last().copied().unwrap_or(0.);
    let failure_json = failure
        .map(|s| format!("{:?}", s))
        .unwrap_or_else(|| "null".to_string());
    println!("{{\"q\":{:?},\"v\":{:?},\"completed\":{},\"completed_microframes\":{},\"failure\":{},\"frames\":{},\"cubic_order\":{},\"energy_j\":{},\"initial_energy_j\":{},\"initial_wire_energy_j\":{:?},\"final_wire_energy_j\":{:?},\"per_wire_energy_constraint\":{},\"work_relative_max\":{},\"raw_local_work_relative_max\":{},\"numerical_lambda_abs_max\":{},\"midpoint_residual_relative_max\":{},\"maximum_iterations\":{},\"allocations\":{},\"seconds\":{},\"buffers\":{},\"mean_ms\":{},\"p99_ms\":{},\"max_ms\":{},\"known_prepared_bytes\":{},\"numerical_energy_constraint\":{},\"forced_axial_waveform\":{},\"high_axial_modes_per_wire\":{},\"contact_included\":false,\"nonlinear_step\":true,\"full_piano_realtime\":false,\"temporal_collocation\":{},\"buffer_microframes\":{},\"profile\":{},\"axial_elimination\":{},\"audited_residual_relative_max\":{},\"observation_samples\":{}}}",model.q,model.v,failure.is_none(),completed,failure_json,frames,order,energy,initial,initial_wires,final_wires,model.per_wire_constraint,work_max,raw_max,lambda_max,residual_max,max_iterations,allocations,elapsed,times.len(),mean,p99,maximum,model.prepare_bytes(),model.constrained,model.forced_axial,model.waves.first().map(|w|w.high_modes()).unwrap_or(0),model.collocation.is_some(),buffer_microframes,model.profile.json(elapsed),model.collocation.as_ref().is_some_and(|c|c.eliminates_axial()),model.collocation.as_ref().map(|c|c.audited_residual_max()).unwrap_or(0.),observation_samples);
}
