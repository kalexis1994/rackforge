//! Optional mechanical diagnostics. No soundboard or acoustic pressure model.
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
};

pub(super) struct Observation {
    path: String,
    width: usize,
    axial: usize,
    weights: Vec<[f64; 3]>,
    samples: Vec<[f64; 10]>,
}
impl Observation {
    pub(super) fn new(
        path: String,
        nt: usize,
        nl: usize,
        length: f64,
        mu: f64,
        steps: usize,
    ) -> Self {
        let root = (mu * length / 2.).sqrt();
        let weights = (1..=nt)
            .map(|n| {
                [0.125, 0.31, 0.73].map(|x| {
                    (std::f64::consts::PI * n as f64 * x).sin()
                        * (-1.2 * (n as f64 * 0.02).powi(2)).exp()
                        / root
                })
            })
            .collect();
        Self {
            path,
            width: nl + 2 * nt,
            axial: nl,
            weights,
            samples: Vec::with_capacity(steps + 1),
        }
    }
    pub(super) fn sample(&mut self, time: f64, v: &[f64]) {
        let mut row = [0.; 10];
        row[0] = time;
        for wire in 0..3 {
            for (k, weights) in self.weights.iter().enumerate() {
                let value = v[wire * self.width + self.axial + k];
                for probe in 0..3 {
                    row[1 + 3 * wire + probe] += weights[probe] * value;
                }
            }
        }
        self.samples.push(row);
    }
    pub(super) fn len(&self) -> usize {
        self.samples.len()
    }
    pub(super) fn write(&self) {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
            .unwrap();
        let mut out = BufWriter::new(file);
        write!(out, "{{\"units\":\"m/s\",\"positions_fraction\":[0.125,0.31,0.73],\"width_fraction\":0.02,\"acoustic_pressure\":false,\"rows\":[").unwrap();
        for (i, row) in self.samples.iter().enumerate() {
            if i != 0 {
                write!(out, ",").unwrap();
            }
            write!(out, "{:?}", row).unwrap();
        }
        writeln!(out, "]}}").unwrap();
        out.flush().unwrap();
    }
}
