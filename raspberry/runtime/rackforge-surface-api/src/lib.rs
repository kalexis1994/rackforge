use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const SURFACE_ACTIVATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceMode {
    Live,
    Play,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceActivationReason {
    ReturnToActiveMode,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceActivationRequest {
    pub schema_version: u32,
    pub layout_id: String,
    pub mode: SurfaceMode,
    pub reason: SurfaceActivationReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_item_id: Option<String>,
}

impl SurfaceActivationRequest {
    pub fn return_to(
        layout_id: impl Into<String>,
        mode: SurfaceMode,
        selected_item_id: Option<String>,
    ) -> Self {
        Self {
            schema_version: SURFACE_ACTIVATION_SCHEMA_VERSION,
            layout_id: layout_id.into(),
            mode,
            reason: SurfaceActivationReason::ReturnToActiveMode,
            selected_item_id,
        }
    }

    pub fn validate(&self) -> Result<(), SurfaceError> {
        if self.schema_version != SURFACE_ACTIVATION_SCHEMA_VERSION {
            return Err(SurfaceError::UnsupportedSchema(self.schema_version));
        }
        validate_layout_id(&self.layout_id)?;
        if self
            .selected_item_id
            .as_ref()
            .is_some_and(|item| item.trim().is_empty() || item.contains('\0'))
        {
            return Err(SurfaceError::InvalidItemId);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceActivationResponse {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_item_id: Option<String>,
}

impl SurfaceActivationResponse {
    pub fn focus(focus_item_id: Option<String>) -> Self {
        Self {
            schema_version: SURFACE_ACTIVATION_SCHEMA_VERSION,
            focus_item_id,
        }
    }

    pub fn validate(&self) -> Result<(), SurfaceError> {
        if self.schema_version != SURFACE_ACTIVATION_SCHEMA_VERSION {
            return Err(SurfaceError::UnsupportedSchema(self.schema_version));
        }
        if self
            .focus_item_id
            .as_ref()
            .is_some_and(|item| item.trim().is_empty() || item.contains('\0'))
        {
            return Err(SurfaceError::InvalidItemId);
        }
        Ok(())
    }
}

fn validate_layout_id(value: &str) -> Result<(), SurfaceError> {
    let Some((name, version)) = value.rsplit_once('@') else {
        return Err(SurfaceError::InvalidLayoutId);
    };
    if name.is_empty()
        || version.is_empty()
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-._".contains(&byte)
        })
        || !version.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(SurfaceError::InvalidLayoutId);
    }
    Ok(())
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum SurfaceError {
    #[error("unsupported surface activation schema {0}")]
    UnsupportedSchema(u32),
    #[error("invalid surface layout id")]
    InvalidLayoutId,
    #[error("surface item id is empty or contains NUL")]
    InvalidItemId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_versioned_activation_exchange() {
        let request = SurfaceActivationRequest::return_to(
            "little@1",
            SurfaceMode::Play,
            Some("custom.user.warm-piano".into()),
        );
        assert_eq!(request.validate(), Ok(()));
        assert_eq!(
            SurfaceActivationResponse::focus(request.selected_item_id).validate(),
            Ok(())
        );
    }

    #[test]
    fn rejects_unversioned_layouts_and_blank_items() {
        let mut request =
            SurfaceActivationRequest::return_to("little", SurfaceMode::Live, Some(" ".into()));
        assert_eq!(request.validate(), Err(SurfaceError::InvalidLayoutId));
        request.layout_id = "little@1".into();
        assert_eq!(request.validate(), Err(SurfaceError::InvalidItemId));
    }
}
