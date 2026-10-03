//! Native P1b experiment. The inherited horizontal state is an energy
//! coordinate. Its isometric lift H_i = H / sqrt(N) preserves stored energy,
//! with conjugate bridge weights h / sqrt(N), not h copied onto every wire.
//! This is an equivalence convention, not a measurement of horizontal mobility.
use crate::math::sqrtf;

pub const HORIZONTAL_LANES: [usize; 3] = [3, 5, 6];

pub fn common(states: &[f32], mask: u8) -> f32 {
    let mut sum = 0.0;
    for (wire, lane) in HORIZONTAL_LANES.iter().enumerate() {
        if mask & (1 << wire) != 0 {
            sum += states[*lane];
        }
    }
    sum / sqrtf((mask & 7).count_ones() as f32)
}

pub fn bridge_delta(
    s: &mut [f32],
    c: &mut [f32],
    mask: u8,
    horizontal: f32,
    drain: f32,
    phase: f32,
) {
    let hs = common(s, mask);
    let hc = common(c, mask);
    // Match the inherited projection's fixed summation order.
    let zs = s[0] + s[1] + s[2] + horizontal * hs;
    let zc = c[0] + c[1] + c[2] + horizontal * hc;
    let ds = drain * zs + phase * zc;
    let dc = drain * zc - phase * zs;
    let h = horizontal / sqrtf((mask & 7).count_ones() as f32);
    for (wire, lane) in HORIZONTAL_LANES.iter().enumerate() {
        if mask & (1 << wire) != 0 {
            s[wire] -= ds;
            c[wire] -= dc;
            s[*lane] -= h * ds;
            c[*lane] -= h * dc;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isometric_lift_preserves_energy_force_and_complex_loading() {
        for count in 1..=3 {
            let mask = (1 << count) - 1;
            let root = sqrtf(count as f32);
            let mut old_s = [0.3, -0.1, 0.7, -0.4, 0.0];
            let mut old_c = [-0.2, 0.9, 0.1, 0.8, 0.0];
            for wire in count..3 {
                old_s[wire] = 0.0;
                old_c[wire] = 0.0;
            }
            let mut s = [0.0; 7];
            let mut c = [0.0; 7];
            s[..3].copy_from_slice(&old_s[..3]);
            c[..3].copy_from_slice(&old_c[..3]);
            for lane in HORIZONTAL_LANES.iter().take(count) {
                s[*lane] = old_s[3] / root;
                c[*lane] = old_c[3] / root;
            }
            let energy = |s: &[f32], c: &[f32]| s.iter().chain(c).map(|x| x * x).sum::<f32>();
            let before = energy(&s, &c);
            assert!((before - energy(&old_s, &old_c)).abs() < 2e-6);
            let (drain, phase) =
                crate::bridge::complex_loading(5.0, 40.0, count as f32 + 0.0144, 44100.0);
            let zs = old_s[..3].iter().sum::<f32>() + 0.12 * old_s[3];
            let zc = old_c[..3].iter().sum::<f32>() + 0.12 * old_c[3];
            let (ds, dc) = (drain * zs + phase * zc, drain * zc - phase * zs);
            for wire in 0..count {
                old_s[wire] -= ds;
                old_c[wire] -= dc;
            }
            old_s[3] -= 0.12 * ds;
            old_c[3] -= 0.12 * dc;
            bridge_delta(&mut s, &mut c, mask, 0.12, drain, phase);
            assert!(energy(&s, &c) <= before + 2e-6);
            for wire in 0..3 {
                assert!((s[wire] - old_s[wire]).abs() < 2e-6);
                assert!((c[wire] - old_c[wire]).abs() < 2e-6);
            }
            assert!((common(&s, mask) - old_s[3]).abs() < 2e-6);
            assert!((common(&c, mask) - old_c[3]).abs() < 2e-6);
        }
    }

    #[test]
    fn common_bridge_does_not_load_horizontal_differences() {
        let mut s = [0.0; 7];
        let mut c = [0.0; 7];
        s[3] = 0.7;
        s[5] = -0.7;
        c[3] = -0.2;
        c[5] = 0.2;
        let before = (s, c);
        bridge_delta(&mut s, &mut c, 7, 0.12, 0.03, 0.1);
        assert_eq!((s, c), before);
        let matrix = crate::spatial_unison::Directional::new(
            [20.0, -7.0, 5.0],
            [-300.0, 40.0, 50.0],
            3.0,
            0.02,
            44100.0,
        );
        assert_eq!(matrix.split_moment(&s, 7), 0.0);
        matrix.apply_split(&mut s, &mut c, 7);
        assert_eq!((s, c), before);
    }

    #[test]
    fn split_partial_tracks_the_aggregate_with_and_without_directional_loading() {
        use crate::{Component, Partial};
        for count in 1..=3 {
            for lever in [0.0, 0.025 / 1.4] {
                let mut old = Partial::default();
                old.bridge_mask = (1 << count) - 1;
                for wire in 0..count {
                    old.set_lane(
                        wire,
                        Component::start_state(
                            0.1 * (wire + 1) as f32,
                            -0.3,
                            500.0,
                            0.99999,
                            44100.0,
                        ),
                    );
                }
                old.set_lane(
                    3,
                    Component::start_state(-0.2, 0.4, 500.0, 0.99999, 44100.0),
                );
                (old.drain, old.bridge_phase) =
                    crate::bridge::complex_loading(5.0, 40.0, count as f32 + 0.0144, 44100.0);
                if lever > 0.0 {
                    old.directional = crate::spatial_unison::Directional::new(
                        [20.0, -7.0, 5.0],
                        [-300.0, 40.0, 50.0],
                        count as f32,
                        lever,
                        44100.0,
                    );
                }
                let mut split = old;
                split.split_horizontal(1, 0.12, 0.99999);
                let mut max_error = 0.0f32;
                for _ in 0..10000 {
                    max_error = max_error.max((old.tick(0.12) - split.tick(0.12)).abs());
                    assert!((old.moment_out - split.moment_out).abs() < 1e-5);
                }
                assert!(
                    max_error < 5e-5,
                    "count {count}, lever {lever}, error {max_error}"
                );
                assert!((split.horizontal_energy() - old.horizontal_energy()).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn per_wire_initialization_uses_existing_horizontal_energy_and_string_pitch() {
        use crate::{Component, Partial};
        let mut p = Partial::default();
        p.bridge_mask = 7;
        for wire in 0..3 {
            p.set_lane(
                wire,
                Component::start_state(
                    0.13 * (wire + 1) as f32,
                    -0.24,
                    600.0 + 0.1 * wire as f32,
                    0.99998,
                    44100.0,
                ),
            );
        }
        p.set_lane(
            3,
            Component::start_state(-0.2, 0.4, 600.0, 0.99999, 44100.0),
        );
        let old_energy = p.horizontal_energy();
        let old_output = p.s[..3].iter().sum::<f32>() + 0.12 * p.s[3] + p.s[4];
        let angles: Vec<f32> = (0..3).map(|i| p.rs[i].atan2(p.rc[i])).collect();
        p.split_horizontal(2, 0.12, 0.99999);
        assert!((p.horizontal_energy() - old_energy).abs() < 2e-6);
        assert!(
            (p.s[..3].iter().sum::<f32>() + 0.12 * p.horizontal_common() + p.s[4] - old_output)
                .abs()
                < 2e-6
        );
        for (wire, lane) in HORIZONTAL_LANES.iter().enumerate() {
            assert!((p.rs[*lane].atan2(p.rc[*lane]) - angles[wire]).abs() < 2e-7);
            assert!(
                (p.rc[*lane] * p.rc[*lane] + p.rs[*lane] * p.rs[*lane] - 0.99999f32.powi(2)).abs()
                    < 2e-7
            );
        }
    }

    #[test]
    fn horizontal_differences_survive_culling_and_follow_horizontal_damper_grip() {
        use crate::{Partial, Voice};
        let mut voice = Voice::<1>::default();
        voice.note = 60;
        voice.active = true;
        voice.partial_count = 1;
        voice.pole_ceiling = 0.999999;
        let mut p = Partial::default();
        p.bridge_mask = 7;
        p.polarization_mask = 7;
        p.s[5] = 0.2;
        p.s[6] = -0.2;
        for lane in HORIZONTAL_LANES {
            p.rc[lane] = 0.99999;
        }
        voice.partials[0] = p;
        assert_eq!(voice.cull(), 0);
        assert!((voice.energy - 0.08).abs() < 2e-6);
        voice.damp(0.98, 0.0, 1.0, 0.0);
        assert_eq!(voice.partials[0].rc[3], voice.partials[0].rc[5]);
        assert_eq!(voice.partials[0].rc[5], voice.partials[0].rc[6]);
        voice.lift_seated_damper();
        for lane in HORIZONTAL_LANES {
            assert!((voice.partials[0].rc[lane] - 0.99999).abs() < 2e-7);
        }
    }
}
