use crate::PARAMETER_SCHEMA_VERSION;
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
}

impl ParameterSchema {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.schema_version != PARAMETER_SCHEMA_VERSION {
            return Err(SchemaError::UnsupportedSchema(self.schema_version));
        }
        let mut page_ids = BTreeSet::new();
        for page in &self.pages {
            validate_identifier(&page.id)?;
            if page.name.trim().is_empty() {
                return Err(SchemaError::EmptyName(page.id.clone()));
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
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageDescriptor {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub order: i32,
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
    #[error("invalid identifier {0:?}")]
    InvalidIdentifier(String),
    #[error("empty display name for {0}")]
    EmptyName(String),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_declarative_pages_and_parameters() {
        let schema = ParameterSchema {
            schema_version: 1,
            pages: vec![PageDescriptor {
                id: "level".into(),
                name: "Level".into(),
                order: 0,
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
        };
        assert_eq!(schema.validate(), Ok(()));
    }
}
