//! Research: one hammer head, three stiff strings and three felt histories.
//! No bridge feedback during contact; subsequent bridge loading stays in Partial.
use crate::math::{expf, powf, sincosf, sqrtf};
use crate::{SIM_MODES, StrikeConfiguration, contact_pulse, unison};

pub(crate) struct Strike {
    pub q: [[f32; SIM_MODES]; 3],
    pub over_omega: [[f32; SIM_MODES]; 3],
    pub pulse: contact_pulse::ContactPulse,
    pub ended: bool,
    pub duration_s: f32,
    pub impulse_ns: [f32; 3],
    pub energy_in: f64,
    pub energy_out: f64,
}

pub(crate) fn heights(note: u8, span: f32) -> [f32; 3] {
    let mut gaps = core::array::from_fn(|i| {
        if i == 0 {
            0.0
        } else {
            span * (2.0 * crate::hash01((note as u32) << 5 | (i as u32) << 2 | 0xB) - 1.0)
        }
    });
    let closest = gaps.iter().copied().fold(f32::INFINITY, f32::min);
    for gap in &mut gaps {
        *gap -= closest;
    }
    gaps
}

pub(crate) fn strike(
    frequencies: &[f32],
    modes: usize,
    c: StrikeConfiguration,
    tuning: unison::Unison,
    gaps: [f32; 3],
    areas: [f32; 3],
    dt: f32,
    capture: bool,
) -> Strike {
    let dt = dt.clamp(1e-7, 4e-6);
    let steps = ((c.contact_seconds / dt) as usize).clamp(200, 200000);
    let mut pulse = contact_pulse::ContactPulse::new(if capture {
        steps as f64 * dt as f64
    } else {
        0.0
    });
    let mut omega = [[0.0; SIM_MODES]; 3];
    let mut shape = [0.0; SIM_MODES];
    for n in 0..modes {
        let h = (n + 1) as f32;
        let ideal = sincosf(core::f32::consts::PI * h * c.x0).0;
        let floor = crate::comb_floor_at(n + 1, c.comb_floor);
        let comb = if ideal < 0.0 { -1.0 } else { 1.0 } * sqrtf(ideal * ideal + floor * floor);
        let width = h * c.contact_width;
        shape[n] = comb * expf(-1.2 * width * width);
        let ratios = tuning.ratios(n + 1);
        for i in 0..3 {
            omega[i][n] = core::f32::consts::TAU * frequencies[n] * ratios[i];
        }
    }
    let mut q = [[0.0; SIM_MODES]; 3];
    let mut v = [[0.0; SIM_MODES]; 3];
    let (mut hammer_y, mut hammer_v) = (0.0, c.velocity);
    let mut memory = [0.0; 3];
    let keep = expf(-dt / c.stulov_tau);
    let (mut touched, mut ended, mut duration) = (false, false, 0.0);
    let mut impulse = [0.0; 3];
    for step in 0..steps {
        let mut forces = [0.0; 3];
        let mut contact = false;
        for i in 0..3 {
            if areas[i] <= 0.0 {
                continue;
            }
            let string_y = (0..modes).map(|n| q[i][n] * shape[n]).sum::<f32>();
            let compression = hammer_y - gaps[i] - string_y;
            if compression > 0.0 {
                contact = true;
                touched = true;
                let compressed = powf(compression, c.exponent);
                memory[i] = keep * memory[i] + (1.0 - keep) * compressed;
                forces[i] =
                    (areas[i] * c.stiffness * (compressed - c.stulov_epsilon * memory[i])).max(0.0);
            } else {
                memory[i] = 0.0;
            }
            impulse[i] += forces[i] * dt;
        }
        if touched && !contact {
            ended = true;
            break;
        }
        duration = (step + 1) as f32 * dt;
        if capture {
            // Preserve the previous calibrated per-string force convention.
            // Physical total force is 3 times this; the case gain is effective.
            pulse.deposit(
                step as f64 * dt as f64,
                (step + 1) as f64 * dt as f64,
                forces.iter().sum::<f32>() / 3.0,
            );
        }
        hammer_v -= forces.iter().sum::<f32>() / (3.0 * c.mass) * dt;
        hammer_y += hammer_v * dt;
        for i in 0..3 {
            for n in 0..modes {
                v[i][n] += (-omega[i][n] * omega[i][n] * q[i][n]
                    + 2.0 * shape[n] * forces[i] / c.string_mass)
                    * dt;
                q[i][n] += v[i][n] * dt;
            }
        }
    }
    let mut over_omega = [[0.0; SIM_MODES]; 3];
    let square = |x: f64| x * x;
    let mut energy_out = 0.5 * (3.0 * c.mass) as f64 * square(hammer_v as f64);
    for i in 0..3 {
        for n in 0..modes {
            over_omega[i][n] = v[i][n] / omega[i][n].max(1.0);
            energy_out += 0.25
                * c.string_mass as f64
                * (square(v[i][n] as f64) + square(omega[i][n] as f64 * q[i][n] as f64));
        }
    }
    Strike {
        q,
        over_omega,
        pulse,
        ended,
        duration_s: duration,
        impulse_ns: impulse,
        energy_in: 0.5 * (3.0 * c.mass) as f64 * square(c.velocity as f64),
        energy_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> StrikeConfiguration {
        StrikeConfiguration {
            x0: 0.12,
            contact_width: 0.02,
            mass: 0.0015,
            string_mass: 0.006,
            stiffness: 4e8,
            exponent: 2.5,
            velocity: 2.0,
            contact_seconds: 0.02,
            stulov_epsilon: 0.0,
            stulov_tau: 0.0001,
            comb_floor: 0.02,
        }
    }
    #[test]
    fn common_head_preserves_symmetry_and_has_no_spontaneous_energy() {
        let frequencies = [440.0, 880.0, 1320.0, 1760.0];
        let tuning = unison::Unison::new(69, 0.0, 0.0, 0.0);
        for dt in [2e-6, 1e-6, 5e-7] {
            let r = strike(
                &frequencies,
                4,
                config(),
                tuning,
                [0.0; 3],
                [1.0; 3],
                dt,
                false,
            );
            assert!(r.ended && r.duration_s > 0.0001 && r.duration_s < 0.02);
            assert_eq!(r.q[0], r.q[1]);
            assert_eq!(r.q[1], r.q[2]);
            assert_eq!(r.impulse_ns[0], r.impulse_ns[1]);
            assert!(
                (r.energy_out / r.energy_in - 1.0).abs() < 0.01,
                "{}",
                r.energy_out / r.energy_in
            );
        }
    }
    #[test]
    fn three_contacts_exchange_energy_through_the_same_head() {
        let tuning = unison::Unison::new(84, 0.0, 0.2, 0.0);
        let gaps = heights(84, 20e-6);
        let r = strike(
            &[1046.5, 2093.0, 3139.5],
            3,
            config(),
            tuning,
            gaps,
            [1.0; 3],
            1e-6,
            false,
        );
        assert!(r.ended && r.impulse_ns.iter().all(|x| *x > 0.0));
        assert_ne!(r.q[0][0], r.q[1][0]);
        assert!((r.energy_out / r.energy_in - 1.0).abs() < 0.01);
        // Initial clearance prevents any preloaded felt impulse at time zero.
        assert!(gaps.iter().all(|x| *x >= 0.0));
        assert_eq!(gaps.iter().copied().fold(f32::INFINITY, f32::min), 0.0);
    }
    #[test]
    fn a_missed_string_receives_no_force_or_energy() {
        let tuning = unison::Unison::new(84, 0.0, 0.0, 0.0);
        let r = strike(
            &[1046.5, 2093.0, 3139.5],
            3,
            config(),
            tuning,
            [0.0; 3],
            [1.0, 1.0, 0.0],
            1e-6,
            false,
        );
        assert!(r.ended);
        assert_eq!(r.q[2], [0.0; SIM_MODES]);
        assert_eq!(r.over_omega[2], [0.0; SIM_MODES]);
        assert_eq!(r.impulse_ns[2], 0.0);
        assert!((r.energy_out / r.energy_in - 1.0).abs() < 0.01);
    }
}
