/// Fixed host gain at the end of an instrument's PLAY chain, before the master.
#[derive(Clone, Copy)]
pub struct PlayOutputTrim(f32);

impl PlayOutputTrim {
    pub const UNITY: Self = Self(1.0);

    pub fn for_instrument(plugin_id: &str) -> Self {
        match plugin_id {
            "org.rackforge.musette" => Self(1.090_184_5), // +0.75 dB
            "org.rackforge.organ" => Self(1.258_925_4),   // +2.0 dB
            "org.rackforge.rftines" => Self(1.011_579_5), // +0.1 dB
            _ => Self::UNITY,
        }
    }

    pub fn apply(self, output: &mut [f32]) {
        if self.0 != 1.0 {
            for sample in output {
                *sample *= self.0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PlayOutputTrim;

    #[test]
    fn applies_the_calibrated_gain_linearly_and_preserves_unity() {
        let block = [0.125, -0.25, -0.0, 1.0];
        for (id, decibels) in [
            ("org.rackforge.concert-grand", 0.0),
            ("org.rackforge.rf-5", 0.0),
            ("org.rackforge.musette", 0.75),
            ("org.rackforge.organ", 2.0),
            ("org.rackforge.rf-epiano", 0.0),
            ("org.rackforge.rftines", 0.1),
        ] {
            let mut output = block;
            PlayOutputTrim::for_instrument(id).apply(&mut output);
            let measured_db = 20.0 * output[3].log10();
            assert!((measured_db - decibels).abs() < 1e-5, "{id}");
            assert_eq!(
                output.map(f32::to_bits),
                block.map(|sample| (sample * output[3]).to_bits())
            );
        }
    }
}
