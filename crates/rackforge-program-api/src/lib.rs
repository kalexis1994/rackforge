use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use thiserror::Error;

pub const PROGRAM_SCHEMA_VERSION: u32 = 1;
pub const PROGRAM_EDIT_SCHEMA_VERSION: u32 = 1;
pub const PROGRAM_EDITOR_SCHEMA_VERSION: u32 = 1;
pub const MAX_PROGRAM_ARTIFACTS: usize = 16;
pub const MAX_PROGRAM_ARTIFACT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDocument {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub plugin_id: String,
    pub plugin_version: String,
    pub plugin_state_version: u32,
    pub payload_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramEditRequest {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program_id: Option<String>,
}

impl ProgramEditRequest {
    pub fn new(program_id: Option<String>) -> Self {
        Self {
            schema_version: PROGRAM_EDIT_SCHEMA_VERSION,
            program_id,
        }
    }

    pub fn validate(&self) -> Result<(), ProgramError> {
        if self.schema_version != PROGRAM_EDIT_SCHEMA_VERSION {
            return Err(ProgramError::UnsupportedEditSchema(self.schema_version));
        }
        if let Some(program_id) = &self.program_id {
            validate_program_identifier(program_id.strip_prefix("custom.").unwrap_or(program_id))?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedProgram {
    pub schema_version: u32,
    pub storage_path: String,
    pub preview_sound_id: String,
    pub document: ProgramDocument,
    /// Small plugin-owned files derived from the document at save time.
    ///
    /// Examples include hardware SysEx exports and compact lookup indexes.
    /// RackForge stores them in the same plugin namespace as the Program and
    /// never interprets their contents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ProgramArtifact>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramArtifact {
    pub storage_path: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

/// A resolved, platform-neutral editor tree published by a plugin for one
/// concrete program draft. Surfaces render this tree without knowing the
/// plugin payload shape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramEditorView {
    pub schema_version: u32,
    pub title: String,
    pub pages: Vec<ProgramEditorPage>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramEditorPage {
    pub id: String,
    pub label: String,
    pub detail: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<ProgramEditorPage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<ProgramEditorField>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramEditorField {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub value: ProgramEditorValue,
    pub kind: ProgramEditorFieldKind,
    #[serde(default)]
    pub live_preview: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProgramEditorFieldKind {
    Toggle,
    Number {
        minimum: i64,
        maximum: i64,
        step: i64,
        #[serde(default)]
        decimals: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        #[serde(default)]
        allow_inherited: bool,
    },
    Choice {
        options: Vec<ProgramEditorChoice>,
    },
    Sound {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bank: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramEditorChoice {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ProgramEditorValue {
    Inherited,
    Boolean(bool),
    Integer(i64),
    Choice(String),
    SoundId(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramFieldEditRequest {
    pub schema_version: u32,
    pub document: ProgramDocument,
    pub field_id: String,
    pub value: ProgramEditorValue,
}

impl ProgramEditorView {
    pub fn validate(&self) -> Result<(), ProgramError> {
        if self.schema_version != PROGRAM_EDITOR_SCHEMA_VERSION {
            return Err(ProgramError::UnsupportedEditorSchema(self.schema_version));
        }
        validate_editor_text(&self.title)?;
        if self.pages.is_empty() {
            return Err(ProgramError::EmptyEditor);
        }
        let mut ids = BTreeSet::new();
        for page in &self.pages {
            page.validate(&mut ids)?;
        }
        Ok(())
    }
}

impl ProgramEditorPage {
    fn validate(&self, ids: &mut BTreeSet<String>) -> Result<(), ProgramError> {
        validate_editor_id(&self.id)?;
        if !ids.insert(self.id.clone()) {
            return Err(ProgramError::DuplicateEditorIdentifier(self.id.clone()));
        }
        validate_editor_text(&self.label)?;
        validate_editor_text(&self.detail)?;
        if self.pages.is_empty() && self.fields.is_empty() {
            return Err(ProgramError::EmptyEditorPage(self.id.clone()));
        }
        for page in &self.pages {
            page.validate(ids)?;
        }
        for field in &self.fields {
            field.validate(ids)?;
        }
        Ok(())
    }
}

impl ProgramEditorField {
    fn validate(&self, ids: &mut BTreeSet<String>) -> Result<(), ProgramError> {
        validate_editor_id(&self.id)?;
        if !ids.insert(self.id.clone()) {
            return Err(ProgramError::DuplicateEditorIdentifier(self.id.clone()));
        }
        validate_editor_text(&self.label)?;
        validate_editor_text(&self.detail)?;
        match (&self.kind, &self.value) {
            (ProgramEditorFieldKind::Toggle, ProgramEditorValue::Boolean(_)) => {}
            (
                ProgramEditorFieldKind::Number {
                    minimum,
                    maximum,
                    step,
                    decimals,
                    unit,
                    allow_inherited,
                },
                value,
            ) => {
                if minimum > maximum || *step <= 0 || *decimals > 6 {
                    return Err(ProgramError::InvalidEditorField(self.id.clone()));
                }
                if unit
                    .as_ref()
                    .is_some_and(|unit| validate_editor_text(unit).is_err())
                {
                    return Err(ProgramError::InvalidEditorField(self.id.clone()));
                }
                match value {
                    ProgramEditorValue::Integer(value) if (*minimum..=*maximum).contains(value) => {
                    }
                    ProgramEditorValue::Inherited if *allow_inherited => {}
                    _ => return Err(ProgramError::InvalidEditorField(self.id.clone())),
                }
            }
            (ProgramEditorFieldKind::Choice { options }, ProgramEditorValue::Choice(value)) => {
                if options.is_empty()
                    || !options.iter().any(|option| option.value == *value)
                    || options.iter().any(|option| {
                        validate_editor_id(&option.value).is_err()
                            || validate_editor_text(&option.label).is_err()
                            || option
                                .detail
                                .as_ref()
                                .is_some_and(|detail| validate_editor_text(detail).is_err())
                    })
                {
                    return Err(ProgramError::InvalidEditorField(self.id.clone()));
                }
            }
            (ProgramEditorFieldKind::Sound { bank }, ProgramEditorValue::SoundId(value)) => {
                if value.trim().is_empty()
                    || value.contains('\0')
                    || bank
                        .as_ref()
                        .is_some_and(|bank| validate_editor_id(bank).is_err())
                {
                    return Err(ProgramError::InvalidEditorField(self.id.clone()));
                }
            }
            _ => return Err(ProgramError::InvalidEditorField(self.id.clone())),
        }
        Ok(())
    }
}

impl ProgramFieldEditRequest {
    pub fn validate(&self) -> Result<(), ProgramError> {
        if self.schema_version != PROGRAM_EDITOR_SCHEMA_VERSION {
            return Err(ProgramError::UnsupportedEditorSchema(self.schema_version));
        }
        self.document.validate()?;
        validate_editor_id(&self.field_id)
    }
}

fn default_true() -> bool {
    true
}

fn validate_editor_id(value: &str) -> Result<(), ProgramError> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(ProgramError::InvalidEditorIdentifier(value.to_owned()));
    }
    Ok(())
}

fn validate_editor_text(value: &str) -> Result<(), ProgramError> {
    if value.trim().is_empty() || value.contains('\0') || !value.is_ascii() || value.len() > 64 {
        return Err(ProgramError::InvalidEditorText);
    }
    Ok(())
}

impl PreparedProgram {
    pub fn validate(&self) -> Result<(), ProgramError> {
        if self.schema_version != PROGRAM_EDIT_SCHEMA_VERSION {
            return Err(ProgramError::UnsupportedEditSchema(self.schema_version));
        }
        if self.storage_path.trim().is_empty()
            || self.storage_path.contains('\0')
            || self.preview_sound_id.trim().is_empty()
            || self.preview_sound_id.contains('\0')
        {
            return Err(ProgramError::InvalidEditMetadata);
        }
        self.document.validate()?;
        if self.artifacts.len() > MAX_PROGRAM_ARTIFACTS {
            return Err(ProgramError::TooManyArtifacts(self.artifacts.len()));
        }
        let mut paths = BTreeSet::new();
        let mut total_bytes = 0usize;
        for artifact in &self.artifacts {
            artifact.validate()?;
            if artifact.storage_path == self.storage_path
                || !paths.insert(artifact.storage_path.clone())
            {
                return Err(ProgramError::DuplicateArtifactPath(
                    artifact.storage_path.clone(),
                ));
            }
            total_bytes = total_bytes
                .checked_add(artifact.bytes.len())
                .ok_or(ProgramError::ArtifactsTooLarge(usize::MAX))?;
        }
        if total_bytes > MAX_PROGRAM_ARTIFACT_BYTES {
            return Err(ProgramError::ArtifactsTooLarge(total_bytes));
        }
        Ok(())
    }
}

impl ProgramArtifact {
    pub fn validate(&self) -> Result<(), ProgramError> {
        if !valid_artifact_path(&self.storage_path) {
            return Err(ProgramError::InvalidArtifactPath(self.storage_path.clone()));
        }
        if self.media_type.trim().is_empty()
            || self.media_type.len() > 128
            || !self.media_type.is_ascii()
            || self.media_type.contains('\0')
        {
            return Err(ProgramError::InvalidArtifactMediaType);
        }
        if self.bytes.is_empty() {
            return Err(ProgramError::EmptyArtifact(self.storage_path.clone()));
        }
        Ok(())
    }
}

fn valid_artifact_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\0')
        && !value.contains('\\')
        && !value.starts_with('/')
        && !value.contains(':')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

impl ProgramDocument {
    pub fn validate(&self) -> Result<(), ProgramError> {
        if self.schema_version != PROGRAM_SCHEMA_VERSION {
            return Err(ProgramError::UnsupportedSchema(self.schema_version));
        }
        validate_program_identifier(&self.id)?;
        validate_plugin_identifier(&self.plugin_id)?;
        if self.name.trim().is_empty() || self.name.contains('\0') {
            return Err(ProgramError::InvalidName);
        }
        if Version::parse(&self.plugin_version).is_err() {
            return Err(ProgramError::InvalidPluginVersion(
                self.plugin_version.clone(),
            ));
        }
        if self.plugin_state_version == 0 {
            return Err(ProgramError::InvalidPluginStateVersion);
        }
        if self.payload_version == 0 {
            return Err(ProgramError::InvalidPayloadVersion);
        }
        if self
            .category
            .as_ref()
            .is_some_and(|category| category.trim().is_empty() || category.contains('\0'))
        {
            return Err(ProgramError::InvalidCategory);
        }
        let mut tags = BTreeSet::new();
        for tag in &self.tags {
            let normalized = tag.trim();
            if normalized.is_empty()
                || normalized.contains('\0')
                || !tags.insert(normalized.to_lowercase())
            {
                return Err(ProgramError::InvalidTag(tag.clone()));
            }
        }
        if !self.payload.is_object() {
            return Err(ProgramError::PayloadMustBeObject);
        }
        Ok(())
    }
}

pub fn validate_program_identifier(value: &str) -> Result<(), ProgramError> {
    validate_identifier(value, false)
        .map_err(|_| ProgramError::InvalidProgramIdentifier(value.to_owned()))
}

pub fn validate_plugin_identifier(value: &str) -> Result<(), ProgramError> {
    validate_identifier(value, true)
        .map_err(|_| ProgramError::InvalidPluginIdentifier(value.to_owned()))
}

fn validate_identifier(value: &str, require_dot: bool) -> Result<(), ()> {
    if value.is_empty()
        || (require_dot && !value.contains('.'))
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(());
    }
    Ok(())
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ProgramError {
    #[error("unsupported program schema {0}")]
    UnsupportedSchema(u32),
    #[error("invalid program identifier {0:?}")]
    InvalidProgramIdentifier(String),
    #[error("invalid plugin identifier {0:?}")]
    InvalidPluginIdentifier(String),
    #[error("program name is empty or contains NUL")]
    InvalidName,
    #[error("invalid plugin version {0:?}")]
    InvalidPluginVersion(String),
    #[error("plugin state version must be positive")]
    InvalidPluginStateVersion,
    #[error("payload version must be positive")]
    InvalidPayloadVersion,
    #[error("program category is empty or contains NUL")]
    InvalidCategory,
    #[error("invalid or duplicate program tag {0:?}")]
    InvalidTag(String),
    #[error("program payload must be a JSON object")]
    PayloadMustBeObject,
    #[error("unsupported program edit schema {0}")]
    UnsupportedEditSchema(u32),
    #[error("program edit metadata is empty or contains NUL")]
    InvalidEditMetadata,
    #[error("prepared program has {0} artifacts, exceeding the host limit")]
    TooManyArtifacts(usize),
    #[error("prepared program artifacts total {0} bytes, exceeding the host limit")]
    ArtifactsTooLarge(usize),
    #[error("invalid program artifact path {0:?}")]
    InvalidArtifactPath(String),
    #[error("program artifact media type is empty, non-ASCII, too long, or contains NUL")]
    InvalidArtifactMediaType,
    #[error("program artifact {0:?} is empty")]
    EmptyArtifact(String),
    #[error("duplicate or colliding program artifact path {0:?}")]
    DuplicateArtifactPath(String),
    #[error("unsupported program editor schema {0}")]
    UnsupportedEditorSchema(u32),
    #[error("program editor is empty")]
    EmptyEditor,
    #[error("program editor page {0:?} is empty")]
    EmptyEditorPage(String),
    #[error("invalid program editor identifier {0:?}")]
    InvalidEditorIdentifier(String),
    #[error("duplicate program editor identifier {0:?}")]
    DuplicateEditorIdentifier(String),
    #[error("program editor text is empty, non-ASCII, too long, or contains NUL")]
    InvalidEditorText,
    #[error("invalid program editor field {0:?}")]
    InvalidEditorField(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_program() -> ProgramDocument {
        ProgramDocument {
            schema_version: 1,
            id: "user.piano-strings".into(),
            name: "Piano + Strings".into(),
            plugin_id: "org.rackforge.roland-scva".into(),
            plugin_version: "0.1.0".into(),
            plugin_state_version: 2,
            payload_version: 1,
            category: Some("Layered".into()),
            tags: vec!["Piano".into(), "Strings".into()],
            payload: json!({"layers": []}),
        }
    }

    #[test]
    fn validates_generic_program_envelope() {
        assert_eq!(valid_program().validate(), Ok(()));
    }

    #[test]
    fn rejects_paths_and_duplicate_case_insensitive_tags() {
        let mut program = valid_program();
        program.id = "../escape".into();
        assert!(matches!(
            program.validate(),
            Err(ProgramError::InvalidProgramIdentifier(_))
        ));

        let mut program = valid_program();
        program.tags = vec!["Piano".into(), "piano".into()];
        assert!(matches!(
            program.validate(),
            Err(ProgramError::InvalidTag(_))
        ));
    }

    #[test]
    fn validates_program_edit_contracts() {
        let request = ProgramEditRequest::new(Some("custom.user.piano-strings".into()));
        assert_eq!(request.validate(), Ok(()));
        let prepared = PreparedProgram {
            schema_version: PROGRAM_EDIT_SCHEMA_VERSION,
            storage_path: "custom/user.piano-strings.rackforge-program.json".into(),
            preview_sound_id: "dls.b00000000.p00000000".into(),
            document: valid_program(),
            artifacts: vec![ProgramArtifact {
                storage_path: "exports/user.piano-strings.syx".into(),
                media_type: "audio/midi".into(),
                bytes: vec![0xf0, 0xf7],
            }],
        };
        assert_eq!(prepared.validate(), Ok(()));
    }

    #[test]
    fn rejects_unsafe_or_oversized_program_artifacts() {
        let mut prepared = PreparedProgram {
            schema_version: PROGRAM_EDIT_SCHEMA_VERSION,
            storage_path: "programs/test.rackforge-program.json".into(),
            preview_sound_id: "custom.test".into(),
            document: valid_program(),
            artifacts: vec![ProgramArtifact {
                storage_path: "../escape.syx".into(),
                media_type: "audio/midi".into(),
                bytes: vec![0xf0, 0xf7],
            }],
        };
        assert!(matches!(
            prepared.validate(),
            Err(ProgramError::InvalidArtifactPath(_))
        ));

        prepared.artifacts[0].storage_path = "exports/test.syx".into();
        prepared.artifacts[0].bytes = vec![0; MAX_PROGRAM_ARTIFACT_BYTES + 1];
        assert!(matches!(
            prepared.validate(),
            Err(ProgramError::ArtifactsTooLarge(_))
        ));
    }

    #[test]
    fn validates_declarative_editor_and_typed_mutation() {
        let view = ProgramEditorView {
            schema_version: PROGRAM_EDITOR_SCHEMA_VERSION,
            title: "RF-DLS".into(),
            pages: vec![ProgramEditorPage {
                id: "layer-a".into(),
                label: "LAYER A".into(),
                detail: "Required layer".into(),
                enabled: true,
                pages: Vec::new(),
                fields: vec![ProgramEditorField {
                    id: "layer.a.gain".into(),
                    label: "VOLUME".into(),
                    detail: "Layer mix gain".into(),
                    value: ProgramEditorValue::Integer(100),
                    kind: ProgramEditorFieldKind::Number {
                        minimum: 0,
                        maximum: 200,
                        step: 1,
                        decimals: 2,
                        unit: None,
                        allow_inherited: false,
                    },
                    live_preview: true,
                }],
            }],
        };
        assert_eq!(view.validate(), Ok(()));

        let edit = ProgramFieldEditRequest {
            schema_version: PROGRAM_EDITOR_SCHEMA_VERSION,
            document: valid_program(),
            field_id: "layer.a.gain".into(),
            value: ProgramEditorValue::Integer(80),
        };
        assert_eq!(edit.validate(), Ok(()));
    }
}
