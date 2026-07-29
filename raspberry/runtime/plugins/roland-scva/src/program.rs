use rackforge_plugin_api::ProgramDocument;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PAYLOAD_VERSION: u32 = 1;
pub const MAX_LAYERS: usize = 2;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RolandProgram {
    pub master_gain: f32,
    pub layers: Vec<ProgramLayer>,
}

impl RolandProgram {
    pub fn from_document(document: &ProgramDocument) -> Result<Self, String> {
        document.validate().map_err(|error| error.to_string())?;
        if document.plugin_id != "org.rackforge.roland-scva" {
            return Err("program belongs to a different plugin".into());
        }
        if document.payload_version != PAYLOAD_VERSION {
            return Err(format!(
                "unsupported Roland program payload {}",
                document.payload_version
            ));
        }
        let program: Self = serde_json::from_value(document.payload.clone())
            .map_err(|error| format!("parsing Roland program: {error}"))?;
        program.validate()?;
        Ok(program)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.master_gain.is_finite() || !(0.0..=2.0).contains(&self.master_gain) {
            return Err("master gain must be between 0 and 2".into());
        }
        if self.layers.is_empty() || self.layers.len() > MAX_LAYERS {
            return Err("a Roland program must contain one or two layers".into());
        }
        if !self.layers.iter().any(|layer| layer.enabled) {
            return Err("a Roland program must enable at least one layer".into());
        }
        let mut ids = BTreeSet::new();
        for layer in &self.layers {
            layer.validate()?;
            if !ids.insert(layer.id.as_str()) {
                return Err(format!("duplicate layer id {:?}", layer.id));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramLayer {
    pub id: String,
    pub enabled: bool,
    pub sound_id: String,
    pub gain: f32,
    pub pan: f32,
    pub octave: i8,
    pub transpose_semitones: i8,
    pub fine_tune_cents: i16,
    pub key_range: MidiRange,
    pub velocity_range: MidiRange,
    pub envelope: Envelope,
}

impl ProgramLayer {
    fn validate(&self) -> Result<(), String> {
        if !matches!(self.id.as_str(), "a" | "b") {
            return Err("layer id must be \"a\" or \"b\"".into());
        }
        if self.sound_id.is_empty()
            || self.sound_id.starts_with('.')
            || self.sound_id.ends_with('.')
            || self.sound_id.contains("..")
            || !self.sound_id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
            })
        {
            return Err(format!("invalid sound id {:?}", self.sound_id));
        }
        if !self.gain.is_finite() || !(0.0..=2.0).contains(&self.gain) {
            return Err(format!("layer {:?} has invalid gain", self.id));
        }
        if !self.pan.is_finite() || !(-1.0..=1.0).contains(&self.pan) {
            return Err(format!("layer {:?} has invalid pan", self.id));
        }
        if !(-4..=4).contains(&self.octave)
            || !(-12..=12).contains(&self.transpose_semitones)
            || !(-100..=100).contains(&self.fine_tune_cents)
        {
            return Err(format!("layer {:?} has invalid tuning", self.id));
        }
        self.key_range.validate("key", &self.id)?;
        self.velocity_range.validate("velocity", &self.id)?;
        self.envelope.validate(&self.id)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MidiRange {
    pub low: u8,
    pub high: u8,
}

impl MidiRange {
    fn validate(&self, name: &str, layer: &str) -> Result<(), String> {
        if self.low > self.high || self.high > 127 {
            return Err(format!("layer {layer:?} has invalid {name} range"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Envelope {
    fn validate(&self, layer: &str) -> Result<(), String> {
        if !self.attack.is_finite()
            || !self.decay.is_finite()
            || !self.sustain.is_finite()
            || !self.release.is_finite()
            || !(0.0..=5.0).contains(&self.attack)
            || !(0.0..=5.0).contains(&self.decay)
            || !(0.0..=1.0).contains(&self.sustain)
            || !(0.005..=10.0).contains(&self.release)
        {
            return Err(format!("layer {layer:?} has invalid envelope"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_piano_is_a_valid_single_layer_program() {
        let document: ProgramDocument =
            serde_json::from_slice(include_bytes!("../programs/factory.piano-1.json")).unwrap();
        let program = RolandProgram::from_document(&document).unwrap();
        assert_eq!(program.layers.len(), 1);
        assert_eq!(program.layers[0].id, "a");
        assert_eq!(program.layers[0].sound_id, "scva.piano-1");
    }

    #[test]
    fn factory_strings_is_a_valid_single_layer_program() {
        let document: ProgramDocument =
            serde_json::from_slice(include_bytes!("../programs/factory.strings-1.json")).unwrap();
        let program = RolandProgram::from_document(&document).unwrap();
        assert_eq!(program.layers.len(), 1);
        assert_eq!(program.layers[0].id, "a");
        assert_eq!(program.layers[0].sound_id, "scva.strings-1");
    }

    #[test]
    fn refuses_more_than_two_layers() {
        let document: ProgramDocument =
            serde_json::from_slice(include_bytes!("../programs/factory.piano-1.json")).unwrap();
        let mut program = RolandProgram::from_document(&document).unwrap();
        program.layers.push(program.layers[0].clone());
        program.layers[1].id = "b".into();
        program.layers.push(program.layers[0].clone());
        assert!(program.validate().is_err());
    }
}
