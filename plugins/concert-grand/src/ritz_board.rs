//! Native P2 low-band research asset. The file contains mass-normalized Ritz
//! shapes and poles, with matched bridge source/load samples and surface grid.
//! The inherited upper bank is retained; this is not a full new soundboard.
use std::sync::OnceLock;

pub const CUTOFF: f32 = 800.0;
pub struct Mode {
    pub frequency: f32,
    pub bridge: [f32; 16],
    pub cosine: [f32; 16],
    pub surface: [f32; 960],
}
pub struct Bank {
    pub modes: Vec<Mode>,
}
static BANK: OnceLock<Option<Bank>> = OnceLock::new();

pub fn bank() -> Option<&'static Bank> {
    BANK.get_or_init(|| {
        let path=std::env::var("CG_RITZ_BOARD").ok()?;
        let text=std::fs::read_to_string(path).expect("Ritz bank CSV");
        let mut lines=text.lines();
        assert_eq!(lines.next(),Some("# CG_RITZ_V1 cutoff=800 bridge_points=16 surface=40x24 length=1.4 width=1.0 bridge_inset=0.1..0.9"));
        let mut modes=Vec::new();
        let mut previous=0.0;
        for line in lines {
            let values:Vec<f32>=line.split(',').map(|x|x.parse::<f32>().unwrap()).collect();
            assert_eq!(values.len(),993);
            assert!(values.iter().all(|x|x.is_finite()));
            let f=values[0];
            assert!(f>previous && f<CUTOFF); previous=f;
            let mut mode=Mode { frequency:f, bridge:[0.0;16], cosine:[0.0;16], surface:[0.0;960] };
            mode.bridge.copy_from_slice(&values[1..17]);
            mode.cosine.copy_from_slice(&values[17..33]);
            mode.surface.copy_from_slice(&values[33..]);
            modes.push(mode);
        }
        assert!(!modes.is_empty() && modes.len()<256);
        Some(Bank { modes })
    }).as_ref()
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "run in a fresh process with CG_RITZ_BOARD structural CSV"]
    fn native_bank_uses_the_same_bridge_shapes_for_source_and_loading() {
        use crate::{math::sincosf, ConcertGrand, BOARD_DRIVE_POINTS};
        let bank = super::bank().expect("structural asset");
        let mut piano = Box::new(ConcertGrand::default());
        assert!(piano.prepare(44100.0, 256, 0, 2));
        assert_eq!(
            piano.board_cold[..piano.board_count]
                .iter()
                .filter(|c| c.ritz_index > 0)
                .count(),
            bank.modes.len()
        );
        assert!(piano.board_count > bank.modes.len());
        for (i, expected) in bank.modes.iter().enumerate() {
            assert_eq!(piano.engine.bridge_modes[i].inverse_mass, 1.0);
            for point in 0..BOARD_DRIVE_POINTS {
                let position = point as f32 / (BOARD_DRIVE_POINTS - 1) as f32;
                let load = piano.engine.bridge_projections(position)[i];
                let mut cos = [0.0; BOARD_DRIVE_POINTS];
                let mut sin = [0.0; BOARD_DRIVE_POINTS];
                for q in 0..BOARD_DRIVE_POINTS {
                    let (s, c) = sincosf(core::f32::consts::PI * q as f32 * position);
                    cos[q] = c;
                    sin[q] = s;
                }
                let source = piano.board[i].excitation(&cos, &sin);
                assert!((load - expected.bridge[point]).abs() < 2e-6);
                assert!(
                    (source - load).abs() < 2e-5,
                    "mode {i}, point {point}: {source}/{load}"
                );
            }
        }
        for cold in &piano.board_cold[bank.modes.len()..piano.board_count] {
            assert_eq!(cold.ritz_index, 0);
            assert!(cold.omega * 44100.0 / core::f32::consts::TAU >= super::CUTOFF);
        }
        eprintln!(
            "Ritz modes: {}, inherited high modes: {}",
            bank.modes.len(),
            piano.board_count - bank.modes.len()
        );
    }
}
