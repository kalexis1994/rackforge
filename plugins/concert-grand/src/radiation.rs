//! Shared experimental mode shape for bridge excitation and spatial radiation.
//! This is a reduced plate basis, not a measured soundboard or an acoustic solver.
use crate::math::{sincosf, sqrtf};

/// One receiver in soundboard coordinates, including its signed polar axis.
/// The room writes these from its actual capsules; the board must not interpret
/// their radial distance as an unrelated vertical microphone height.
#[derive(Clone, Copy, Default)]
pub(crate) struct Capsule {
    pub position: [f32; 3],
    pub axis: [f32; 3],
}

impl Capsule {
    pub fn observe(self, x: f32, y: f32, pattern: f32) -> (f32, f32) {
        let d = [x-self.position[0], y-self.position[1], -self.position[2]];
        let range = sqrtf(d[0]*d[0]+d[1]*d[1]+d[2]*d[2]).max(1e-3);
        let cosine = (d[0]*self.axis[0]+d[1]*self.axis[1]+d[2]*self.axis[2])/range;
        (range, (1.0-pattern)+pattern*cosine)
    }

    /// A lower bound on paths from any point of the rectangular board.
    /// The pair uses one shared reference, preserving every relative delay.
    pub fn minimum_path(self, length: f32, width: f32) -> f32 {
        let dx = self.position[0]-self.position[0].clamp(0.0, length);
        let dy = self.position[1]-self.position[1].clamp(-0.5*width, 0.5*width);
        sqrtf(dx*dx+dy*dy+self.position[2]*self.position[2]).max(1e-3)
    }
}

/// Retarded Rayleigh pressure from a modal surface velocity. The common
/// minimum-path/c travel time is removed equally from both capsules and every
/// mode (height/c in the historical vertical-pair geometry).
/// This research observer has no acoustic feedback into the mechanical bank.
pub(crate) struct RetardedPressure {
    #[cfg(not(target_arch = "wasm32"))]
    kernel: [Vec<f32>; 2],
    #[cfg(not(target_arch = "wasm32"))]
    history: Vec<f32>,
    // The diagnostic never runs on WASM and occupies no bank storage there.
    #[cfg(target_arch = "wasm32")]
    kernel: [[f32; 0]; 2],
    #[cfg(target_arch = "wasm32")]
    history: [f32; 0],
    previous_velocity: f32,
    cursor: usize,
    end: usize,
}

/// Commute the modal sum and acoustic delays. Each modal acceleration adds
/// its weighted impulse into common delay slots; only the pair needs histories.
/// Independent slot updates can be vectorized without reassociating a mode sum.
#[derive(Default)]
pub(crate) struct PressureSum {
    #[cfg(not(target_arch = "wasm32"))]
    impulse: Vec<[f64; 2]>,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Vec<[f64; 2]>,
    #[cfg(target_arch = "wasm32")]
    impulse: [[f64; 2]; 0],
    #[cfg(target_arch = "wasm32")]
    pending: [[f64; 2]; 0],
    cursor: usize,
    end: usize,
}

impl PressureSum {
    pub fn support(&mut self, required: usize) {
        self.end = self.end.max(required);
        if required <= self.pending.len() { return; }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let capacity = required.next_power_of_two();
            let previous = core::mem::take(&mut self.pending);
            self.pending = vec![[0.0; 2]; capacity];
            for ahead in 0..previous.len() {
                self.pending[ahead] = previous[(self.cursor+ahead)&(previous.len()-1)];
            }
            self.cursor = 0;
            self.impulse.resize(capacity, [0.0; 2]);
        }
        #[cfg(target_arch = "wasm32")]
        panic!("pressure accumulation is a native-only research observer");
    }
    pub fn clear_state(&mut self) {
        self.impulse.fill([0.0; 2]);
        self.pending.fill([0.0; 2]);
        self.cursor = 0;
    }
    pub fn tick(&mut self) -> (f32, f32) {
        if self.pending.is_empty() { return (0.0, 0.0); }
        let mask = self.pending.len()-1;
        for delay in 0..self.end {
            let index = (self.cursor+delay)&mask;
            self.pending[index][0] += self.impulse[delay][0];
            self.pending[index][1] += self.impulse[delay][1];
            self.impulse[delay] = [0.0; 2];
        }
        let out = self.pending[self.cursor];
        self.pending[self.cursor] = [0.0; 2];
        self.cursor = (self.cursor+1)&mask;
        (out[0] as f32, out[1] as f32)
    }
}

impl Default for RetardedPressure {
    fn default() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            kernel: [Vec::new(), Vec::new()],
            #[cfg(not(target_arch = "wasm32"))]
            history: Vec::new(),
            #[cfg(target_arch = "wasm32")]
            kernel: [[]; 2],
            #[cfg(target_arch = "wasm32")]
            history: [],
            previous_velocity: 0.0,
            cursor: 0,
            end: 0,
        }
    }
}

impl RetardedPressure {
    pub fn end(&self) -> usize { self.end }
    pub fn accumulate(&mut self, velocity: f32, rate: f32, sum: &mut PressureSum) {
        let acceleration = (velocity-self.previous_velocity)*rate;
        self.previous_velocity = velocity;
        debug_assert!(sum.impulse.len() >= self.end);
        for ((slot, left), right) in sum.impulse[..self.end].iter_mut()
            .zip(&self.kernel[0][..self.end]).zip(&self.kernel[1][..self.end]) {
            // Spatial lobes can nearly cancel. Accumulate in f64, rounding
            // only the final pressure rather than every intermediate addition.
            slot[0] += acceleration as f64*(*left as f64);
            slot[1] += acceleration as f64*(*right as f64);
        }
    }
    pub fn clear_kernel(&mut self) {
        self.kernel[0].fill(0.0);
        self.kernel[1].fill(0.0);
        self.end = 0;
    }
    pub fn clear_state(&mut self) {
        self.history.fill(0.0);
        self.previous_velocity = 0.0;
        self.cursor = 0;
    }
    /// Build-time allocation only, native research observer. Never allocate in
    /// tick. Power-of-two storage retains fast wraparound at every sample rate.
    fn support(&mut self, required: usize) {
        if required <= self.history.len() { return; }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let capacity = required.next_power_of_two();
            // Kernel building can grow before any audio has run. Preserve a
            // live history in chronological order if a microphone edit grows it.
            let previous = core::mem::take(&mut self.history);
            self.history = vec![0.0; capacity];
            for ago in 1..=previous.len() {
                self.history[capacity-ago] = previous[(self.cursor+previous.len()-ago)&(previous.len()-1)];
            }
            self.cursor = 0;
            self.kernel[0].resize(capacity, 0.0);
            self.kernel[1].resize(capacity, 0.0);
        }
        #[cfg(target_arch = "wasm32")]
        panic!("retarded pressure is a native-only research observer");
    }
    pub fn deposit(&mut self, side: usize, delay: f32, weight: f32) {
        let index = delay as usize;
        self.support(index+2);
        let frac = delay - index as f32;
        self.kernel[side][index] += weight * (1.0 - frac);
        self.kernel[side][index + 1] += weight * frac;
        self.end = self.end.max(index + 2);
    }
    pub fn tick(&mut self, velocity: f32, rate: f32) -> (f32, f32) {
        if self.history.is_empty() { return (0.0, 0.0); }
        let capacity = self.history.len();
        self.history[self.cursor] = (velocity - self.previous_velocity) * rate;
        self.previous_velocity = velocity;
        let mut out = [0.0; 2];
        for delay in 0..self.end {
            let value = self.history[(self.cursor + capacity - delay) & (capacity-1)];
            out[0] += value * self.kernel[0][delay];
            out[1] += value * self.kernel[1][delay];
        }
        self.cursor = (self.cursor + 1) & (capacity-1);
        (out[0], out[1])
    }
    #[cfg(test)]
    fn tick_precise(&mut self, velocity: f32, rate: f32) -> (f64, f64) {
        let mask = self.history.len()-1;
        self.history[self.cursor] = (velocity-self.previous_velocity)*rate;
        self.previous_velocity = velocity;
        let mut out = [0.0f64;2];
        for delay in 0..self.end {
            let value = self.history[(self.cursor+self.history.len()-delay)&mask] as f64;
            out[0] += value*self.kernel[0][delay] as f64;
            out[1] += value*self.kernel[1][delay] as f64;
        }
        self.cursor = (self.cursor+1)&mask;
        (out[0],out[1])
    }
}

#[cfg(test)]
mod retarded_tests {
    use super::*;

    #[test]
    fn shared_pressure_keeps_precision_with_many_cancelling_modes() {
        let mut direct: Vec<_> = (0..128).map(|_|RetardedPressure::default()).collect();
        let mut shared: Vec<_> = (0..128).map(|_|RetardedPressure::default()).collect();
        let mut sum = PressureSum::default();
        for i in 0..128 {
            let sign = if i%2 == 0 { 1.0 } else { -1.0 };
            for observer in [&mut direct[i],&mut shared[i]] {
                observer.deposit(0, 73.25, sign);
                observer.deposit(1, 311.5, -0.7*sign);
                sum.support(observer.end());
            }
        }
        let mut error = 0.0f64;
        let mut reference = 0.0f64;
        for n in 0..2048 {
            let mut expected = [0.0f64;2];
            for i in 0..128 {
                let velocity = (n as f32*(0.01+0.00001*i as f32)).sin();
                let out = direct[i].tick_precise(velocity,44100.0);
                expected[0] += out.0; expected[1] += out.1;
                shared[i].accumulate(velocity,44100.0,&mut sum);
            }
            let actual = sum.tick();
            error += (actual.0 as f64-expected[0]).powi(2)+(actual.1 as f64-expected[1]).powi(2);
            reference += expected[0]*expected[0]+expected[1]*expected[1];
        }
        assert!(reference > 0.0);
        assert!((error/reference).sqrt() < 1e-6);
    }

    #[test]
    fn shared_delays_match_independent_modal_convolutions() {
        for rate in [44100.0,48000.0,96000.0,192000.0] {
            let mut direct: Vec<_> = (0..3).map(|_| RetardedPressure::default()).collect();
            let mut shared: Vec<_> = (0..3).map(|_| RetardedPressure::default()).collect();
            let mut sum = PressureSum::default();
            for i in 0..3 {
                for observer in [&mut direct[i], &mut shared[i]] {
                    observer.deposit(0, (3.25+17.3*i as f32)*rate/44100.0, 0.7-0.5*i as f32);
                    observer.deposit(1, (6.5+31.7*i as f32)*rate/44100.0, -0.3+0.4*i as f32);
                    sum.support(observer.end());
                }
            }
            let mut error = 0.0f64;
            let mut reference = 0.0f64;
            for pass in 0..2 {
                for n in 0..4096 {
                    let mut expected = [0.0f32;2];
                    for i in 0..3 {
                        let v = (n as f32*(0.01+0.03*i as f32)+i as f32).sin();
                        let out = direct[i].tick(v,rate);
                        expected[0] += out.0; expected[1] += out.1;
                        shared[i].accumulate(v,rate,&mut sum);
                    }
                    let actual = sum.tick();
                    error += (actual.0 as f64-expected[0] as f64).powi(2)
                        +(actual.1 as f64-expected[1] as f64).powi(2);
                    reference += (expected[0] as f64).powi(2)+(expected[1] as f64).powi(2);
                }
                assert!((error/reference).sqrt() < 1e-5, "rate={rate} pass={pass}");
                sum.clear_state();
                for observer in direct.iter_mut().chain(&mut shared) { observer.clear_state(); }
            }
        }
    }

    #[test]
    fn resizing_preserves_recent_acceleration_without_changing_pressure() {
        let mut small = RetardedPressure::default();
        let mut growing = RetardedPressure::default();
        for observer in [&mut small, &mut growing] { observer.deposit(0, 2.5, 0.7); }
        for n in 0..1000 {
            if n == 100 { growing.deposit(0, 303.25, 0.0); }
            let v = (n as f32*0.13).sin();
            assert_eq!(small.tick(v, 44100.0), growing.tick(v, 44100.0));
        }
    }

    #[test]
    fn reset_clears_motion_but_preserves_receiver_kernel() {
        let mut observer = RetardedPressure::default();
        observer.deposit(0, 3.25, 0.7);
        let before: Vec<_> = (0..32).map(|n| observer.tick(if n == 0 { 1.0 } else { 0.0 }, 44100.0)).collect();
        observer.clear_state();
        let after: Vec<_> = (0..32).map(|n| observer.tick(if n == 0 { 1.0 } else { 0.0 }, 44100.0)).collect();
        assert_eq!(before, after);
        assert!(after.iter().any(|(l,_)| *l != 0.0));
    }

    #[test]
    fn off_axis_pressure_keeps_delays_beyond_the_old_history() {
        let mut observer = RetardedPressure::default();
        observer.deposit(0, 303.25, 1.0);
        for n in 0..2200 {
            let (left, right) = observer.tick(if n == 0 { 1.0 } else { 0.0 }, 192000.0);
            let expected = match n { 303 => 144000.0, 304 => -96000.0, 305 => -48000.0, _ => 0.0 };
            assert_eq!(left, expected);
            assert_eq!(right, 0.0);
        }
    }

    #[test]
    fn receiver_preserves_rear_lobe_polarity() {
        let capsule = Capsule { position:[0.0,0.0,1.0], axis:[1.0,0.0,0.0] };
        let (_, front) = capsule.observe(1.0, 0.0, 1.0);
        let (_, back) = capsule.observe(-1.0, 0.0, 1.0);
        assert!(front > 0.0 && back < 0.0);
        assert!((front+back).abs() < 1e-6);
        assert_eq!(capsule.observe(-1.0, 0.0, 0.0).1, 1.0);
    }

    #[test]
    fn pressure_matches_discrete_acceleration_and_retarded_phase() {
        for rate in [44100.0f32, 48000.0, 96000.0] {
            for frequency in [130.0f32, 1000.0, 8000.0, 16000.0] {
                let mut observer = RetardedPressure::default();
                observer.deposit(0, 3.25, 0.7);
                observer.deposit(1, 6.5, -0.3);
                let omega = core::f64::consts::TAU * frequency as f64 / rate as f64;
                let mut squared = 0.0;
                let mut reference = 0.0;
                for n in 0..4096 {
                    let velocity = (omega * n as f64).sin() as f32;
                    let actual = observer.tick(velocity, rate);
                    if n > 300 {
                        let acceleration = |d: usize| {
                            rate as f64
                                * ((omega * (n - d) as f64).sin()
                                    - (omega * (n - d - 1) as f64).sin())
                        };
                        let expected = [
                            0.7 * (0.75 * acceleration(3) + 0.25 * acceleration(4)),
                            -0.3 * (0.5 * acceleration(6) + 0.5 * acceleration(7)),
                        ];
                        squared += (actual.0 as f64 - expected[0]).powi(2)
                            + (actual.1 as f64 - expected[1]).powi(2);
                        reference += expected[0].powi(2) + expected[1].powi(2);
                    }
                }
                assert!(
                    (squared / reference).sqrt() < 1e-5,
                    "{frequency} Hz / {rate} Hz"
                );
            }
        }
    }

    #[test]
    fn finite_pressure_has_no_energy_or_dc_without_motion() {
        let mut observer = RetardedPressure::default();
        observer.deposit(0, 2.5, 1.0);
        observer.deposit(1, 9.3, 1.0);
        for n in 0..2048 {
            let (l, r) = observer.tick(0.2, 44100.0);
            if n > 256 {
                assert_eq!((l, r), (0.0, 0.0));
            }
        }
    }
}

/// Real two-tap filter matching G at omega, using the e^(j omega n)
/// convention: G = a + b exp(-j omega), hence b = -Im(G)/sin(omega).
/// The caller's modes are strictly between DC and Nyquist.
pub(crate) fn output_coefficients(gain: (f32, f32), omega: f32) -> (f32, f32) {
    let (sin, cos) = sincosf(omega);
    let denominator = if sin.abs() < 1e-6 { 1e-6 } else { sin };
    let b = -gain.1 / denominator;
    (gain.0 - b * cos, b)
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Shape {
    pub q: f32,
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub qy: f32,
    pub theta_y: f32,
}

impl Shape {
    pub fn at(self, x: f32, y: f32) -> f32 {
        let (s, c) = sincosf(core::f32::consts::PI * self.q * x);
        (self.a + self.b * c + self.c * s) * self.across(y)
    }

    pub fn across(self, y: f32) -> f32 {
        sincosf(core::f32::consts::PI * self.qy * y + self.theta_y).1
    }

    // Mass normalization on the unit rectangle. Unlike microphone-power
    // normalization this cannot undo cancellation at a particular receiver.
    pub fn normalized(mut self) -> Self {
        let mut along_energy = 0.0;
        let mut across_energy = 0.0;
        for x in 0..40 {
            let (s, c) = sincosf(core::f32::consts::PI * self.q * (x as f32 + 0.5) / 40.0);
            let value = self.a + self.b * c + self.c * s;
            along_energy += value * value / 40.0;
        }
        for y in 0..24 {
            let value = self.across((y as f32 + 0.5) / 24.0 - 0.5);
            across_energy += value * value / 24.0;
        }
        let energy = along_energy * across_energy;
        let scale = if energy > 1e-8 {
            1.0 / sqrtf(energy)
        } else {
            0.0
        };
        self.a *= scale;
        self.b *= scale;
        self.c *= scale;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_filter_preserves_propagation_phase_not_its_conjugate() {
        for frequency in [50.0, 261.626, 349.228, 391.995, 4000.0, 8500.0] {
            let omega = core::f32::consts::TAU * frequency / 44100.0;
            let (im, re) = sincosf(-core::f32::consts::TAU * frequency * 0.7 / 343.0);
            let (a, b) = output_coefficients((re, im), omega);
            let (s, c) = sincosf(omega);
            assert!((a + b * c - re).abs() < 2e-5);
            assert!((-b * s - im).abs() < 2e-5, "phase sign at {frequency}");
        }
    }

    #[test]
    fn mass_normalization_preserves_receiver_cancellation() {
        let odd = Shape {
            q: 1.0,
            b: 1.0,
            ..Shape::default()
        }
        .normalized();
        let mut sum = 0.0;
        let mut energy = 0.0;
        for x in 0..400 {
            let value = odd.at((x as f32 + 0.5) / 400.0, 0.0);
            sum += value / 400.0;
            energy += value * value / 400.0;
        }
        assert!(sum.abs() < 1e-5, "a cancelling mode must stay cancelling");
        assert!((energy - 1.0).abs() < 1e-4);
    }

    #[test]
    fn bridge_projection_matches_the_radiating_surface() {
        for q in [0.0, 1.0, 7.0, 15.0] {
            let shape = Shape {
                q,
                a: 0.2,
                b: 0.8,
                c: -0.4,
                qy: 3.1,
                theta_y: 0.7,
            }
            .normalized();
            for i in 0..16 {
                let x = i as f32 / 15.0;
                let (s, c) = sincosf(core::f32::consts::PI * q * x);
                let bridge = shape.across(0.0) * (shape.a + shape.b * c + shape.c * s);
                assert!((bridge - shape.at(x, 0.0)).abs() < 1e-6);
            }
        }
    }
}
