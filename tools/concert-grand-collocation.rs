//! Experimental collocation in the exact linear interaction frame.
//! The full nonlinear gradient is evaluated at every temporal node.
use super::native::Wire;
use super::profile::{self, Profile, Stage};

const CONTRACTION_TILE: usize = 32;

fn legendre(x: f64, order: usize) -> Vec<f64> {
    let mut p = vec![1.; order + 2];
    p[1] = x;
    for k in 2..p.len() {
        p[k] = ((2 * k - 1) as f64 * x * p[k - 1] - (k - 1) as f64 * p[k - 2]) / k as f64;
    }
    p
}

/// Integrals of the cardinal polynomials, using Gauss orthogonality rather
/// than a monomial Vandermonde system. Each row integrates from 0 to t_i.
pub(super) fn integration_matrix(rule: &[(f64, f64)], h: f64) -> Vec<f64> {
    let order = rule.len();
    let polynomials = rule
        .iter()
        .map(|(t, _)| legendre(2. * t / h - 1., order))
        .collect::<Vec<_>>();
    let mut matrix = vec![0.; order * order];
    for i in 0..order {
        for j in 0..order {
            let mut sum = 0.5 * (2. * rule[i].0 / h);
            for k in 1..order {
                sum += 0.5 * polynomials[j][k] * (polynomials[i][k + 1] - polynomials[i][k - 1]);
            }
            matrix[i * order + j] = rule[j].1 * sum;
        }
    }
    matrix
}

pub(super) struct Collocation {
    blocked_contraction: bool,
    legacy_node_updates: bool,
    scalar_forces: bool,
    refresh_converged_forces: bool,
    order: usize,
    size: usize,
    transfer: Vec<[f64; 2]>,
    node_weights: Vec<f64>,
    end_weights: Vec<[f64; 2]>,
    free: Vec<f64>,
    q: Vec<f64>,
    next: Vec<f64>,
    gradient: Vec<f64>,
}
impl Collocation {
    pub(super) fn new(omega2: &[f64], h: f64, order: usize) -> Self {
        assert!(h.is_finite() && h > 0. && (8..=128).contains(&order));
        assert!(omega2.iter().all(|w| w.is_finite() && *w > 0.));
        let size = omega2.len();
        let rule = super::gauss(order, h);
        let matrix = integration_matrix(&rule, h);
        let mut transfer = Vec::with_capacity(order * size);
        let mut node_weights = Vec::with_capacity(order * order * size);
        let mut end_weights = Vec::with_capacity(order * size);
        for &(t, b) in &rule {
            for &w2 in omega2 {
                let w = w2.sqrt();
                let (s, c) = (w * t).sin_cos();
                transfer.push([c, s / w]);
                let (s, c) = (w * (h - t)).sin_cos();
                end_weights.push([-b * s / w, -b * c]);
            }
        }
        for i in 0..order {
            for j in 0..order {
                for &w2 in omega2 {
                    let w = w2.sqrt();
                    node_weights
                        .push(-matrix[i * order + j] * (w * (rule[i].0 - rule[j].0)).sin() / w);
                }
            }
        }
        let mut result = Self {
            blocked_contraction: true,
            legacy_node_updates: false,
            scalar_forces: false,
            refresh_converged_forces: false,
            order,
            size,
            transfer,
            node_weights,
            end_weights,
            free: vec![0.; order * size],
            q: vec![0.; order * size],
            next: vec![0.; order * size],
            gradient: vec![0.; order * size],
        };
        // Store each output tile's temporal weights contiguously. The old
        // table is released during preparation, before processing is counted.
        let mut packed = Vec::with_capacity(result.node_weights.len());
        for row in result.node_weights.chunks_exact(order * size) {
            for base in (0..size).step_by(CONTRACTION_TILE) {
                let lanes = (size - base).min(CONTRACTION_TILE);
                for j in 0..order {
                    packed.extend_from_slice(&row[j * size + base..j * size + base + lanes]);
                }
            }
        }
        result.node_weights = packed;
        result
    }
    pub(super) fn streamed_node_contraction(&mut self) {
        if !self.blocked_contraction {
            return;
        }
        let mut unpacked = vec![0.; self.node_weights.len()];
        for i in 0..self.order {
            for base in (0..self.size).step_by(CONTRACTION_TILE) {
                let lanes = (self.size - base).min(CONTRACTION_TILE);
                for j in 0..self.order {
                    let from = i * self.order * self.size + base * self.order + j * lanes;
                    let to = (i * self.order + j) * self.size + base;
                    unpacked[to..to + lanes]
                        .copy_from_slice(&self.node_weights[from..from + lanes]);
                }
            }
        }
        self.node_weights = unpacked;
        self.blocked_contraction = false;
    }
    fn contract_blocked(&mut self) {
        for i in 0..self.order {
            let free = &self.free[i * self.size..(i + 1) * self.size];
            let out = &mut self.next[i * self.size..(i + 1) * self.size];
            let row =
                &self.node_weights[i * self.order * self.size..(i + 1) * self.order * self.size];
            let full = self.size / CONTRACTION_TILE * CONTRACTION_TILE;
            for base in (0..full).step_by(CONTRACTION_TILE) {
                let mut values = [0.; CONTRACTION_TILE];
                values.copy_from_slice(&free[base..base + CONTRACTION_TILE]);
                let tile = &row[base * self.order..(base + CONTRACTION_TILE) * self.order];
                for (weights, force) in tile
                    .chunks_exact(CONTRACTION_TILE)
                    .zip(self.gradient.chunks_exact(self.size))
                {
                    let force = &force[base..base + CONTRACTION_TILE];
                    for k in 0..CONTRACTION_TILE {
                        // Temporal nodes retain their original summation order.
                        values[k] += weights[k] * force[k];
                    }
                }
                out[base..base + CONTRACTION_TILE].copy_from_slice(&values);
            }
            // Arbitrary modal counts use the same packed layout in the tail.
            if full < self.size {
                let lanes = self.size - full;
                out[full..].copy_from_slice(&free[full..]);
                for (weights, force) in row[full * self.order..]
                    .chunks_exact(lanes)
                    .zip(self.gradient.chunks_exact(self.size))
                {
                    for k in 0..lanes {
                        out[full + k] += weights[k] * force[full + k];
                    }
                }
            }
        }
    }
    pub(super) fn prepared_bytes(&self) -> usize {
        (self.transfer.capacity() + self.end_weights.capacity()) * 16
            + (self.node_weights.capacity()
                + self.free.capacity()
                + self.q.capacity()
                + self.next.capacity()
                + self.gradient.capacity())
                * 8
    }
    fn forces(&mut self, wires: &mut [Wire]) {
        let width = wires[0].size();
        let batch_end = if self.scalar_forces {
            0
        } else {
            self.order / 4 * 4
        };
        for node in (0..batch_end).step_by(4) {
            for (i, wire) in wires.iter_mut().enumerate() {
                let start = node * self.size + i * width;
                wire.point_nonlinearity_batch4(
                    &self.q[start..],
                    self.size,
                    &mut self.gradient[start..],
                );
            }
        }
        for node in batch_end..self.order {
            for (i, wire) in wires.iter_mut().enumerate() {
                let range = node * self.size + i * width..node * self.size + (i + 1) * width;
                wire.point_nonlinearity(&self.q[range.clone()], &mut self.gradient[range]);
            }
        }
    }
    pub(super) fn scalar_forces(&mut self) {
        self.scalar_forces = true;
    }
    pub(super) fn legacy_node_updates(&mut self) {
        self.legacy_node_updates = true;
    }
    pub(super) fn refresh_converged_forces(&mut self) {
        self.refresh_converged_forces = true;
    }
    pub(super) fn solve(
        &mut self,
        wires: &mut [Wire],
        omega2: &[f64],
        q: &[f64],
        v: &[f64],
        dq: &mut [f64],
        dv: &mut [f64],
        limit: usize,
        profile: &mut Profile,
    ) -> Result<(usize, f64), &'static str> {
        assert!(q.len() == self.size && v.len() == self.size && omega2.len() == self.size);
        assert!(dq.len() == self.size && dv.len() == self.size);
        let clock = profile::start();
        let scale = q
            .iter()
            .zip(v)
            .zip(omega2)
            .map(|((q, v), w)| w * q * q + v * v)
            .sum::<f64>()
            .sqrt()
            .max(1e-30);
        for node in 0..self.order {
            for j in 0..self.size {
                let k = node * self.size + j;
                let t = self.transfer[k];
                self.free[k] = t[0] * q[j] + t[1] * v[j];
            }
        }
        self.q.copy_from_slice(&self.free);
        profile.record(Stage::NodeSetup, clock);
        for iteration in 1..=limit {
            let clock = profile::start();
            self.forces(wires);
            profile.record(Stage::Forces, clock);
            let clock = profile::start();
            if self.blocked_contraction {
                self.contract_blocked();
            } else {
                self.next.copy_from_slice(&self.free);
                for i in 0..self.order {
                    let out = &mut self.next[i * self.size..(i + 1) * self.size];
                    for j in 0..self.order {
                        let force = &self.gradient[j * self.size..(j + 1) * self.size];
                        let weights = &self.node_weights[(i * self.order + j) * self.size
                            ..(i * self.order + j + 1) * self.size];
                        for k in 0..self.size {
                            out[k] += weights[k] * force[k];
                        }
                    }
                }
            }
            profile.record(Stage::NodeUpdate, clock);
            let clock = profile::start();
            let residual2 = if self.legacy_node_updates {
                self.next
                    .iter()
                    .zip(&self.q)
                    .enumerate()
                    .map(|(k, (a, b))| omega2[k % self.size] * (a - b) * (a - b))
                    .sum::<f64>()
            } else {
                // Visit nodes and modes in the original summation order.
                // Each node reuses omega2 directly, without per-mode modulo.
                let mut sum = 0.;
                for (next, current) in self
                    .next
                    .chunks_exact(self.size)
                    .zip(self.q.chunks_exact(self.size))
                {
                    for ((a, b), w) in next.iter().zip(current).zip(omega2) {
                        let d = a - b;
                        sum += w * d * d;
                    }
                }
                sum
            };
            let residual = residual2.sqrt() / scale;
            profile.record(Stage::Residual, clock);
            if !residual.is_finite() {
                return Err("nonfinite collocation residual");
            }
            if residual <= 1e-11 {
                // g(q) already belongs to nodes whose fixed-point residual
                // meets the original tolerance. Use that certified iterate.
                // The control performs one extra update and force evaluation.
                if self.refresh_converged_forces {
                    self.q.copy_from_slice(&self.next);
                    let clock = profile::start();
                    self.forces(wires);
                    profile.record(Stage::Forces, clock);
                }
                let clock = profile::start();
                dq.fill(0.);
                dv.fill(0.);
                for node in 0..self.order {
                    for j in 0..self.size {
                        let k = node * self.size + j;
                        dq[j] += self.end_weights[k][0] * self.gradient[k];
                        dv[j] += self.end_weights[k][1] * self.gradient[k];
                    }
                }
                profile.record(Stage::Endpoint, clock);
                return Ok((iteration, residual));
            }
            if self.legacy_node_updates {
                self.q.copy_from_slice(&self.next);
            } else {
                // next is overwritten from free before every update. Swap
                // owned workspaces instead of copying the accepted iterate.
                std::mem::swap(&mut self.q, &mut self.next);
            }
        }
        Err("collocation did not converge")
    }
}
