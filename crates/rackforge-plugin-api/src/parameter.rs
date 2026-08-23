use crate::{MIN_PARAMETER_SCHEMA_VERSION, PARAMETER_SCHEMA_VERSION};
use rackforge_control_profile::SemanticControlId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterSchema {
    pub schema_version: u32,
    #[serde(default)]
    pub pages: Vec<PageDescriptor>,
    #[serde(default)]
    pub parameters: Vec<ParameterDescriptor>,
    /// Optional mappings from RackForge semantic roles to this plugin's public
    /// parameters. These are hints for automatic controller profiles; explicit
    /// user MIDI Links always take priority.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub semantic_controls: Vec<PluginSemanticControl>,
}

impl ParameterSchema {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if !(MIN_PARAMETER_SCHEMA_VERSION..=PARAMETER_SCHEMA_VERSION).contains(&self.schema_version)
        {
            return Err(SchemaError::UnsupportedSchema(self.schema_version));
        }
        if self.schema_version < 2 && !self.semantic_controls.is_empty() {
            return Err(SchemaError::SemanticControlsRequireSchema2);
        }
        let mut page_ids = BTreeSet::new();
        for page in &self.pages {
            validate_identifier(&page.id)?;
            if page.name.trim().is_empty() {
                return Err(SchemaError::EmptyName(page.id.clone()));
            }
            if page
                .header
                .as_ref()
                .is_some_and(|header| header.trim().is_empty() || header.contains('\0'))
            {
                return Err(SchemaError::InvalidHeader(page.id.clone()));
            }
            if !page_ids.insert(page.id.as_str()) {
                return Err(SchemaError::DuplicatePage(page.id.clone()));
            }
        }

        let mut indexes = BTreeSet::new();
        let mut parameter_ids = BTreeSet::new();
        for parameter in &self.parameters {
            validate_identifier(&parameter.id)?;
            if parameter.name.trim().is_empty() {
                return Err(SchemaError::EmptyName(parameter.id.clone()));
            }
            if !indexes.insert(parameter.index) {
                return Err(SchemaError::DuplicateIndex(parameter.index));
            }
            if !parameter_ids.insert(parameter.id.as_str()) {
                return Err(SchemaError::DuplicateParameter(parameter.id.clone()));
            }
            if !page_ids.contains(parameter.page.as_str()) {
                return Err(SchemaError::UnknownPage {
                    parameter: parameter.id.clone(),
                    page: parameter.page.clone(),
                });
            }
            parameter.kind.validate(&parameter.id)?;
        }
        let mut semantic_roles = BTreeSet::new();
        for binding in &self.semantic_controls {
            binding
                .role
                .validate()
                .map_err(|_| SchemaError::InvalidSemanticRole(binding.role.to_string()))?;
            if !semantic_roles.insert(binding.role.as_str()) {
                return Err(SchemaError::DuplicateSemanticRole(binding.role.to_string()));
            }
            let parameter = self
                .parameters
                .iter()
                .find(|parameter| parameter.index == binding.parameter_index)
                .ok_or_else(|| SchemaError::UnknownSemanticParameter {
                    role: binding.role.to_string(),
                    parameter_index: binding.parameter_index,
                })?;
            if parameter.flags.read_only || matches!(parameter.kind, ParameterKind::Meter { .. }) {
                return Err(SchemaError::ReadOnlySemanticParameter {
                    role: binding.role.to_string(),
                    parameter: parameter.id.clone(),
                });
            }
        }
        Ok(())
    }

    pub fn parameter_for_semantic_role(
        &self,
        role: &SemanticControlId,
    ) -> Option<&ParameterDescriptor> {
        let index = self
            .semantic_controls
            .iter()
            .find(|binding| &binding.role == role)?
            .parameter_index;
        self.parameters
            .iter()
            .find(|parameter| parameter.index == index)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSemanticControl {
    pub role: SemanticControlId,
    pub parameter_index: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageDescriptor {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub order: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterDescriptor {
    pub index: u32,
    pub id: String,
    pub name: String,
    pub page: String,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub order: i32,
    pub kind: ParameterKind,
    #[serde(default)]
    pub flags: ParameterFlags,
    #[serde(default)]
    pub suggested_control: SuggestedControl,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParameterKind {
    Float {
        minimum: f64,
        maximum: f64,
        default: f64,
        step: f64,
        #[serde(default)]
        unit: Option<String>,
    },
    Integer {
        minimum: i64,
        maximum: i64,
        default: i64,
        step: i64,
        #[serde(default)]
        unit: Option<String>,
    },
    Boolean {
        default: bool,
    },
    Enum {
        default: u32,
        choices: Vec<EnumChoice>,
    },
    Trigger,
    Meter {
        minimum: f64,
        maximum: f64,
        #[serde(default)]
        unit: Option<String>,
    },
}

impl ParameterKind {
    fn validate(&self, parameter: &str) -> Result<(), SchemaError> {
        match self {
            Self::Float {
                minimum,
                maximum,
                default,
                step,
                ..
            } => {
                if !minimum.is_finite()
                    || !maximum.is_finite()
                    || !default.is_finite()
                    || !step.is_finite()
                    || minimum >= maximum
                    || default < minimum
                    || default > maximum
                    || *step <= 0.0
                {
                    return Err(SchemaError::InvalidRange(parameter.to_owned()));
                }
            }
            Self::Integer {
                minimum,
                maximum,
                default,
                step,
                ..
            } if minimum >= maximum || default < minimum || default > maximum || *step <= 0 => {
                return Err(SchemaError::InvalidRange(parameter.to_owned()));
            }
            Self::Enum { default, choices } => {
                let mut values = BTreeSet::new();
                if choices.is_empty()
                    || !choices.iter().any(|choice| choice.value == *default)
                    || choices
                        .iter()
                        .any(|choice| choice.name.trim().is_empty() || !values.insert(choice.value))
                {
                    return Err(SchemaError::InvalidChoices(parameter.to_owned()));
                }
            }
            Self::Meter {
                minimum, maximum, ..
            } if !minimum.is_finite() || !maximum.is_finite() || minimum >= maximum => {
                return Err(SchemaError::InvalidRange(parameter.to_owned()));
            }
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnumChoice {
    pub value: u32,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterFlags {
    #[serde(default)]
    pub automatable: bool,
    #[serde(default)]
    pub modulatable: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub advanced: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestedControl {
    #[default]
    Automatic,
    Knob,
    Toggle,
    Button,
    List,
    Meter,
}

fn validate_identifier(value: &str) -> Result<(), SchemaError> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(SchemaError::InvalidIdentifier(value.to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SchemaError {
    #[error("unsupported parameter schema {0}")]
    UnsupportedSchema(u32),
    #[error("semantic controls require parameter schema 2")]
    SemanticControlsRequireSchema2,
    #[error("invalid identifier {0:?}")]
    InvalidIdentifier(String),
    #[error("empty display name for {0}")]
    EmptyName(String),
    #[error("invalid optional header for page {0}")]
    InvalidHeader(String),
    #[error("duplicate page {0}")]
    DuplicatePage(String),
    #[error("duplicate parameter {0}")]
    DuplicateParameter(String),
    #[error("duplicate parameter index {0}")]
    DuplicateIndex(u32),
    #[error("parameter {parameter} references unknown page {page}")]
    UnknownPage { parameter: String, page: String },
    #[error("invalid range for parameter {0}")]
    InvalidRange(String),
    #[error("invalid choices for parameter {0}")]
    InvalidChoices(String),
    #[error("invalid semantic control role {0:?}")]
    InvalidSemanticRole(String),
    #[error("duplicate semantic control role {0}")]
    DuplicateSemanticRole(String),
    #[error("semantic role {role} references unknown parameter index {parameter_index}")]
    UnknownSemanticParameter { role: String, parameter_index: u32 },
    #[error("semantic role {role} references read-only parameter {parameter}")]
    ReadOnlySemanticParameter { role: String, parameter: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_declarative_pages_and_parameters() {
        let schema = ParameterSchema {
            schema_version: PARAMETER_SCHEMA_VERSION,
            pages: vec![PageDescriptor {
                id: "level".into(),
                name: "Level".into(),
                order: 0,
                header: Some("Reference Gain".into()),
            }],
            parameters: vec![ParameterDescriptor {
                index: 0,
                id: "gain".into(),
                name: "Gain".into(),
                page: "level".into(),
                group: None,
                order: 0,
                kind: ParameterKind::Float {
                    minimum: 0.0,
                    maximum: 2.0,
                    default: 1.0,
                    step: 0.01,
                    unit: None,
                },
                flags: ParameterFlags::default(),
                suggested_control: SuggestedControl::Knob,
            }],
            semantic_controls: vec![PluginSemanticControl {
                role: SemanticControlId::new("plugin.output.level").unwrap(),
                parameter_index: 0,
            }],
        };
        assert_eq!(schema.validate(), Ok(()));
    }

    #[test]
    fn page_header_is_optional_but_never_blank() {
        let json = r#"{
            "schema_version": 1,
            "pages": [{"id":"sound","name":"Sound","order":0}],
            "parameters": []
        }"#;
        let schema: ParameterSchema = serde_json::from_str(json).unwrap();
        assert_eq!(schema.pages[0].header, None);
        assert_eq!(schema.validate(), Ok(()));

        let mut invalid = schema;
        invalid.pages[0].header = Some(" ".into());
        assert_eq!(
            invalid.validate(),
            Err(SchemaError::InvalidHeader("sound".into()))
        );
    }

    #[test]
    fn semantic_roles_are_unique_and_target_writable_parameters() {
        let mut schema = ParameterSchema {
            schema_version: PARAMETER_SCHEMA_VERSION,
            pages: vec![PageDescriptor {
                id: "main".into(),
                name: "Main".into(),
                order: 0,
                header: None,
            }],
            parameters: vec![ParameterDescriptor {
                index: 7,
                id: "cutoff".into(),
                name: "Cutoff".into(),
                page: "main".into(),
                group: None,
                order: 0,
                kind: ParameterKind::Float {
                    minimum: 0.0,
                    maximum: 1.0,
                    default: 0.5,
                    step: 0.01,
                    unit: None,
                },
                flags: ParameterFlags {
                    automatable: true,
                    ..Default::default()
                },
                suggested_control: SuggestedControl::Knob,
            }],
            semantic_controls: vec![PluginSemanticControl {
                role: SemanticControlId::new("synth.filter.cutoff").unwrap(),
                parameter_index: 7,
            }],
        };
        assert_eq!(schema.validate(), Ok(()));
        assert_eq!(
            schema
                .parameter_for_semantic_role(
                    &SemanticControlId::new("synth.filter.cutoff").unwrap()
                )
                .unwrap()
                .index,
            7
        );
        let mut legacy = schema.clone();
        legacy.schema_version = 1;
        assert_eq!(
            legacy.validate(),
            Err(SchemaError::SemanticControlsRequireSchema2)
        );
        schema
            .semantic_controls
            .push(schema.semantic_controls[0].clone());
        assert!(matches!(
            schema.validate(),
            Err(SchemaError::DuplicateSemanticRole(_))
        ));
    }
}
