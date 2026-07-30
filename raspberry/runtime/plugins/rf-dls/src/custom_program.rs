use rackforge_plugin_api::{
    PROGRAM_EDIT_SCHEMA_VERSION, PROGRAM_SCHEMA_VERSION, PreparedProgram, ProgramDocument,
};
use rf_dls::EnvelopeSpec;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const PLUGIN_ID: &str = "org.rackforge.rf-dls";
pub const PAYLOAD_VERSION: u32 = 1;
pub const PROGRAM_SUFFIX: &str = ".rackforge-program.json";
const MAX_PROGRAM_BYTES: u64 = 256 * 1024;
const MAX_PROGRAMS: usize = 1024;

#[derive(Clone, Debug)]
pub struct CustomProgram {
    pub id: String,
    pub name: String,
    pub category: Option<String>,
    pub slot: u16,
    pub source: DlsSource,
    pub parameters: ProgramParameters,
}

impl CustomProgram {
    pub fn preset_id(&self) -> String {
        format!("custom.{}", self.id)
    }

    pub fn from_document(document: ProgramDocument) -> Result<Self, String> {
        document.validate().map_err(|error| error.to_string())?;
        if document.plugin_id != PLUGIN_ID {
            return Err("program belongs to a different addon".into());
        }
        if !matches!(document.plugin_state_version, 1 | 2) {
            return Err(format!(
                "unsupported RF-DLS state compatibility {}",
                document.plugin_state_version
            ));
        }
        if document.payload_version != PAYLOAD_VERSION {
            return Err(format!(
                "unsupported RF-DLS payload version {}",
                document.payload_version
            ));
        }
        let payload: CustomProgramPayload = serde_json::from_value(document.payload)
            .map_err(|error| format!("parsing RF-DLS payload: {error}"))?;
        payload.validate()?;
        Ok(Self {
            id: document.id,
            name: document.name,
            category: document.category,
            slot: payload.slot,
            source: payload.source,
            parameters: payload.parameters,
        })
    }

    pub fn to_document(&self) -> Result<ProgramDocument, String> {
        Ok(ProgramDocument {
            schema_version: PROGRAM_SCHEMA_VERSION,
            id: self.id.clone(),
            name: self.name.clone(),
            plugin_id: PLUGIN_ID.into(),
            plugin_version: env!("CARGO_PKG_VERSION").into(),
            plugin_state_version: 2,
            payload_version: PAYLOAD_VERSION,
            category: self.category.clone(),
            tags: vec!["custom".into()],
            payload: serde_json::to_value(CustomProgramPayload {
                slot: self.slot,
                source: self.source.clone(),
                parameters: self.parameters,
            })
            .map_err(|error| format!("serializing RF-DLS payload: {error}"))?,
        })
    }

    pub fn prepared(&self) -> Result<PreparedProgram, String> {
        Ok(PreparedProgram {
            schema_version: PROGRAM_EDIT_SCHEMA_VERSION,
            storage_path: format!("custom/{}{}", self.id, PROGRAM_SUFFIX),
            preview_sound_id: format!("dls.b{:08x}.p{:08x}", self.source.bank, self.source.program),
            document: self.to_document()?,
        })
    }

    pub fn resolved_envelope(&self, inherited: EnvelopeSpec) -> EnvelopeSpec {
        EnvelopeSpec {
            attack_seconds: self
                .parameters
                .attack_seconds
                .unwrap_or(inherited.attack_seconds),
            decay_seconds: self
                .parameters
                .decay_seconds
                .unwrap_or(inherited.decay_seconds),
            sustain_level: self
                .parameters
                .sustain_level
                .unwrap_or(inherited.sustain_level),
            release_seconds: self
                .parameters
                .release_seconds
                .unwrap_or(inherited.release_seconds),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustomProgramPayload {
    pub slot: u16,
    pub source: DlsSource,
    #[serde(default)]
    pub parameters: ProgramParameters,
}

impl CustomProgramPayload {
    fn validate(&self) -> Result<(), String> {
        if self.slot == 0 || self.slot > 999 {
            return Err("CUSTOM slot must be between 1 and 999".into());
        }
        if self.source.resource_id != "dls-bank" {
            return Err("CUSTOM source resource_id must be \"dls-bank\"".into());
        }
        self.parameters.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DlsSource {
    pub resource_id: String,
    pub bank: u32,
    pub program: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProgramParameters {
    pub gain: f32,
    pub transpose_semitones: i8,
    pub fine_tune_cents: f32,
    pub attack_seconds: Option<f32>,
    pub decay_seconds: Option<f32>,
    pub sustain_level: Option<f32>,
    pub release_seconds: Option<f32>,
    pub pitch_bend_range_semitones: f32,
    pub modulation_depth: f32,
}

impl Default for ProgramParameters {
    fn default() -> Self {
        Self {
            gain: 1.0,
            transpose_semitones: 0,
            fine_tune_cents: 0.0,
            attack_seconds: None,
            decay_seconds: None,
            sustain_level: None,
            release_seconds: None,
            pitch_bend_range_semitones: 2.0,
            modulation_depth: 1.0,
        }
    }
}

impl ProgramParameters {
    fn validate(&self) -> Result<(), String> {
        finite_range(self.gain, 0.0, 2.0, "gain")?;
        if !(-48..=48).contains(&self.transpose_semitones) {
            return Err("transpose_semitones must be between -48 and 48".into());
        }
        finite_range(self.fine_tune_cents, -100.0, 100.0, "fine_tune_cents")?;
        optional_range(self.attack_seconds, 0.0, 60.0, "attack_seconds")?;
        optional_range(self.decay_seconds, 0.0, 60.0, "decay_seconds")?;
        optional_range(self.sustain_level, 0.0, 1.0, "sustain_level")?;
        optional_range(self.release_seconds, 0.0, 60.0, "release_seconds")?;
        finite_range(
            self.pitch_bend_range_semitones,
            0.0,
            24.0,
            "pitch_bend_range_semitones",
        )?;
        finite_range(self.modulation_depth, 0.0, 2.0, "modulation_depth")
    }
}

fn finite_range(value: f32, minimum: f32, maximum: f32, name: &str) -> Result<(), String> {
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(format!("{name} must be between {minimum} and {maximum}"));
    }
    Ok(())
}

fn optional_range(
    value: Option<f32>,
    minimum: f32,
    maximum: f32,
    name: &str,
) -> Result<(), String> {
    value.map_or(Ok(()), |value| finite_range(value, minimum, maximum, name))
}

pub fn load_programs(
    data_root: Option<&Path>,
) -> Result<(Vec<CustomProgram>, Vec<String>), String> {
    let Some(data_root) = data_root else {
        return Ok((Vec::new(), Vec::new()));
    };
    let directory = data_root.join("custom");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("creating {}: {error}", directory.display()))?;
    let metadata = fs::symlink_metadata(&directory)
        .map_err(|error| format!("inspecting {}: {error}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("RF-DLS custom storage must be a real directory".into());
    }
    let canonical_root = fs::canonicalize(&directory)
        .map_err(|error| format!("resolving {}: {error}", directory.display()))?;
    let mut paths = fs::read_dir(&directory)
        .map_err(|error| format!("reading {}: {error}", directory.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(PROGRAM_SUFFIX))
        })
        .collect::<Vec<PathBuf>>();
    paths.sort();
    if paths.len() > MAX_PROGRAMS {
        return Err(format!(
            "RF-DLS custom storage contains more than {MAX_PROGRAMS} programs"
        ));
    }

    let mut programs = Vec::new();
    let mut warnings = Vec::new();
    let mut ids = BTreeSet::new();
    let mut slots = BTreeSet::new();
    for path in paths {
        match load_one(&canonical_root, &path) {
            Ok(program) if !ids.insert(program.id.clone()) => warnings.push(format!(
                "ignoring {}: duplicate CUSTOM id {:?}",
                path.display(),
                program.id
            )),
            Ok(program) if !slots.insert(program.slot) => warnings.push(format!(
                "ignoring {}: duplicate CUSTOM slot {}",
                path.display(),
                program.slot
            )),
            Ok(program) => programs.push(program),
            Err(error) => warnings.push(format!("ignoring {}: {error}", path.display())),
        }
    }
    programs.sort_by_key(|program| (program.slot, program.id.clone()));
    Ok((programs, warnings))
}

fn load_one(root: &Path, path: &Path) -> Result<CustomProgram, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("inspecting file: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("CUSTOM program must be a regular non-symlink file".into());
    }
    if metadata.len() > MAX_PROGRAM_BYTES {
        return Err(format!(
            "CUSTOM program exceeds the {MAX_PROGRAM_BYTES}-byte limit"
        ));
    }
    let canonical = fs::canonicalize(path).map_err(|error| format!("resolving file: {error}"))?;
    if !canonical.starts_with(root) {
        return Err("CUSTOM program escapes its private addon directory".into());
    }
    let bytes = fs::read(&canonical).map_err(|error| format!("reading file: {error}"))?;
    let document: ProgramDocument =
        serde_json::from_slice(&bytes).map_err(|error| format!("parsing JSON: {error}"))?;
    CustomProgram::from_document(document)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn document() -> ProgramDocument {
        ProgramDocument {
            schema_version: 1,
            id: "user.warm-piano".into(),
            name: "Warm Piano".into(),
            plugin_id: PLUGIN_ID.into(),
            plugin_version: "0.1.0".into(),
            plugin_state_version: 2,
            payload_version: PAYLOAD_VERSION,
            category: Some("Piano".into()),
            tags: vec!["custom".into()],
            payload: json!({
                "slot": 1,
                "source": {
                    "resource_id": "dls-bank",
                    "bank": 0,
                    "program": 0
                },
                "parameters": {
                    "gain": 0.9,
                    "release_seconds": 1.2
                }
            }),
        }
    }

    #[test]
    fn parses_versioned_custom_program() {
        let program = CustomProgram::from_document(document()).unwrap();
        assert_eq!(program.preset_id(), "custom.user.warm-piano");
        assert_eq!(program.slot, 1);
        assert_eq!(program.parameters.transpose_semitones, 0);
        assert_eq!(program.parameters.release_seconds, Some(1.2));
        assert_eq!(
            CustomProgram::from_document(program.to_document().unwrap())
                .unwrap()
                .id,
            "user.warm-piano"
        );
    }

    #[test]
    fn rejects_invalid_source_and_ranges() {
        let mut invalid_source = document();
        invalid_source.payload["source"]["resource_id"] = json!("something-else");
        assert!(CustomProgram::from_document(invalid_source).is_err());

        let mut invalid_gain = document();
        invalid_gain.payload["parameters"]["gain"] = json!(9.0);
        assert!(CustomProgram::from_document(invalid_gain).is_err());
    }
}
