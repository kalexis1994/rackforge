//! Research-only feed-forward contact excitation. Bins contain impulse (N s),
//! not peak-normalized audio. Integration at both ends preserves impulse when
//! the simulation and output sample rates differ. No allocation on note-on.
const BINS: usize = 512;

#[derive(Clone, Copy)]
pub struct ContactPulse {
    impulse: [f32; BINS],
    duration: f64,
    cursor: f64,
    frame: f64,
    keep: f32,
    low1: f32,
    low2: f32,
}

impl Default for ContactPulse {
    fn default() -> Self {
        Self {
            impulse: [0.0; BINS],
            duration: 0.0,
            cursor: 0.0,
            frame: 0.0,
            keep: 0.0,
            low1: 0.0,
            low2: 0.0,
        }
    }
}

impl ContactPulse {
    pub fn active(&self) -> bool {
        self.frame != 0.0
    }

    pub fn new(duration: f64) -> Self {
        Self {
            duration,
            ..Self::default()
        }
    }

    pub fn deposit(&mut self, start: f64, end: f64, force: f32) {
        if self.duration <= 0.0 || end <= start {
            return;
        }
        let width = self.duration / BINS as f64;
        let first = (start.max(0.0) / width) as usize;
        let last = ((end / width) as usize).min(BINS - 1);
        for i in first..=last {
            let overlap = (end.min((i + 1) as f64 * width) - start.max(i as f64 * width)).max(0.0);
            self.impulse[i] += force * overlap as f32;
        }
    }

    fn integral(&self, start: f64, end: f64) -> f64 {
        if self.duration <= 0.0 || end <= start || start >= self.duration {
            return 0.0;
        }
        let width = self.duration / BINS as f64;
        let first = (start.max(0.0) / width) as usize;
        let last = ((end / width) as usize).min(BINS - 1);
        let mut value = 0.0;
        for i in first..=last {
            let overlap = (end.min((i + 1) as f64 * width) - start.max(i as f64 * width)).max(0.0);
            value += self.impulse[i] as f64 * overlap / width;
        }
        value
    }

    pub fn configure(&mut self, rate: f32, gain: f32, corner: f32) {
        for x in &mut self.impulse {
            *x *= gain;
        }
        self.frame = 1.0 / rate as f64;
        self.keep = super::expf(-core::f32::consts::TAU * corner / rate);
    }

    /// Sum a restrike with the pending impulse and keep the transmission's
    /// filter state. The new grid spans both tails; neither hit is discarded.
    pub fn merge(&mut self, fresh: Self) {
        if fresh.frame == 0.0 {
            return;
        }
        if self.frame == 0.0 {
            *self = fresh;
            return;
        }
        let duration = (self.duration - self.cursor).max(fresh.duration);
        let mut merged = Self::new(duration);
        merged.frame = fresh.frame;
        merged.keep = fresh.keep;
        merged.low1 = self.low1;
        merged.low2 = self.low2;
        let width = duration / BINS as f64;
        for i in 0..BINS {
            let a = i as f64 * width;
            let b = (i + 1) as f64 * width;
            merged.impulse[i] =
                (self.integral(self.cursor + a, self.cursor + b) + fresh.integral(a, b)) as f32;
        }
        *self = merged;
    }

    pub fn tick(&mut self) -> f32 {
        if self.frame == 0.0 {
            return 0.0;
        }
        let force = (self.integral(self.cursor, self.cursor + self.frame) / self.frame) as f32;
        self.cursor = (self.cursor + self.frame).min(self.duration);
        // A bounded, unity-DC transmission filter, not an identified case model.
        self.low1 = self.keep * self.low1 + (1.0 - self.keep) * force;
        self.low2 = self.keep * self.low2 + (1.0 - self.keep) * self.low1;
        if self.cursor >= self.duration && self.low1.abs() + self.low2.abs() < 1e-20 {
            self.low1 = 0.0;
            self.low2 = 0.0;
            self.frame = 0.0;
        }
        self.low2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pulse() -> ContactPulse {
        let mut p = ContactPulse::new(0.020);
        for i in 0..400 {
            p.deposit(i as f64 * 4e-6, (i + 1) as f64 * 4e-6, 10.0);
        }
        p
    }
    #[test]
    fn impulse_survives_resampling_and_filtering() {
        for rate in [44100.0, 48000.0, 96000.0, 192000.0] {
            let mut p = pulse();
            p.configure(rate, 0.3, 350.0);
            let sum: f64 = (0..(rate * 0.1) as usize)
                .map(|_| p.tick() as f64 / rate as f64)
                .sum();
            assert!((sum / 0.0048 - 1.0).abs() < 2e-5, "{rate}: {sum}");
        }
    }
    #[test]
    fn overlapping_restrikes_preserve_pending_impulse() {
        let rate = 44100.0;
        let mut p = pulse();
        p.configure(rate, 1.0, 350.0);
        let mut sum = 0.0f64;
        for _ in 0..16 {
            for _ in 0..17 {
                sum += p.tick() as f64 / rate as f64;
            }
            let mut fresh = pulse();
            fresh.configure(rate, 1.0, 350.0);
            p.merge(fresh);
        }
        for _ in 0..4410 {
            sum += p.tick() as f64 / rate as f64;
        }
        assert!((sum / (17.0 * 0.016) - 1.0).abs() < 2e-5, "{sum}");
    }
}
