use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const LITTLE_V1: &str = "little@1";
pub const LITTLE_TEXT_COLUMNS: usize = 18;
pub const LITTLE_BODY_ROWS: usize = 2;
pub const LITTLE_SOFT_KEYS: usize = 4;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceContract {
    pub id: String,
    pub text_columns: u16,
    pub header_rows: u8,
    pub body_rows: u8,
    pub soft_keys: u8,
    pub navigation: NavigationCapabilities,
}

impl SurfaceContract {
    pub fn little_v1() -> Self {
        Self {
            id: LITTLE_V1.into(),
            text_columns: LITTLE_TEXT_COLUMNS as u16,
            header_rows: 1,
            body_rows: LITTLE_BODY_ROWS as u8,
            soft_keys: LITTLE_SOFT_KEYS as u8,
            navigation: NavigationCapabilities {
                previous: true,
                next: true,
                confirm: true,
                back: true,
            },
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_versioned_id(&self.id)?;
        if self.text_columns == 0
            || self.body_rows == 0
            || self.soft_keys == 0
            || !self.navigation.is_complete()
        {
            return Err(format!("surface {:?} has incomplete capabilities", self.id));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationCapabilities {
    pub previous: bool,
    pub next: bool,
    pub confirm: bool,
    pub back: bool,
}

impl NavigationCapabilities {
    pub fn is_complete(self) -> bool {
        self.previous && self.next && self.confirm && self.back
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceQuality {
    Native,
    CertifiedCompatibility,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GestureCapabilities {
    pub soft_key_long_press: bool,
    pub emergency_home_chord: bool,
}

impl GestureCapabilities {
    pub fn validate(self) -> Result<(), String> {
        if self.emergency_home_chord && !self.soft_key_long_press {
            return Err("emergency home chord requires soft-key long press detection".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceImplementation {
    pub layout_id: String,
    pub quality: SurfaceQuality,
    pub priority: u16,
    #[serde(default)]
    pub gestures: GestureCapabilities,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerProfile {
    pub id: String,
    pub name: String,
    pub driver_id: String,
    pub surfaces: Vec<SurfaceImplementation>,
}

impl ControllerProfile {
    pub fn validate(&self) -> Result<(), String> {
        validate_identifier(&self.id)?;
        validate_identifier(&self.driver_id)?;
        if self.name.trim().is_empty() || self.surfaces.is_empty() {
            return Err("controller profile needs a name and at least one surface".into());
        }
        let mut ids = BTreeSet::new();
        let mut priorities = BTreeSet::new();
        for surface in &self.surfaces {
            validate_versioned_id(&surface.layout_id)?;
            surface.gestures.validate()?;
            if !ids.insert(surface.layout_id.as_str()) {
                return Err(format!("duplicate surface {:?}", surface.layout_id));
            }
            if !priorities.insert(surface.priority) {
                return Err(format!("duplicate surface priority {}", surface.priority));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NegotiatedSurface {
    pub layout_id: String,
    pub quality: SurfaceQuality,
}

/// Chooses only an implementation explicitly declared by both sides.
///
/// Screen size, control count and layout names are never inferred.
pub fn negotiate_surface(
    controller: &ControllerProfile,
    addon_layouts: &[String],
) -> Option<NegotiatedSurface> {
    controller
        .surfaces
        .iter()
        .filter(|surface| addon_layouts.contains(&surface.layout_id))
        .min_by_key(|surface| surface.priority)
        .map(|surface| NegotiatedSurface {
            layout_id: surface.layout_id.clone(),
            quality: surface.quality,
        })
}

pub trait ControllerDriver: Sync {
    fn profile(&self) -> &ControllerProfile;
    fn matches_display_output(&self, port_name: &str) -> bool;
    fn matches_surface_input(&self, port_name: &str) -> bool;
}

fn validate_identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('.')
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        })
    {
        return Err(format!("invalid identifier {value:?}"));
    }
    Ok(())
}

fn validate_versioned_id(value: &str) -> Result<(), String> {
    let (id, version) = value
        .rsplit_once('@')
        .ok_or_else(|| format!("surface id {value:?} is not versioned"))?;
    validate_identifier(id)?;
    if version.is_empty() || !version.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("invalid surface version in {value:?}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn medium_controller(with_little: bool) -> ControllerProfile {
        let mut surfaces = vec![SurfaceImplementation {
            layout_id: "medium@1".into(),
            quality: SurfaceQuality::Native,
            priority: 0,
            gestures: GestureCapabilities::default(),
        }];
        if with_little {
            surfaces.push(SurfaceImplementation {
                layout_id: LITTLE_V1.into(),
                quality: SurfaceQuality::CertifiedCompatibility,
                priority: 1,
                gestures: GestureCapabilities::default(),
            });
        }
        ControllerProfile {
            id: "example.medium".into(),
            name: "Example Medium".into(),
            driver_id: "org.rackforge.example-medium".into(),
            surfaces,
        }
    }

    #[test]
    fn never_infers_little_from_a_larger_display() {
        let controller = medium_controller(false);
        assert_eq!(negotiate_surface(&controller, &[LITTLE_V1.into()]), None);
    }

    #[test]
    fn uses_only_explicit_certified_compatibility() {
        let controller = medium_controller(true);
        assert_eq!(
            negotiate_surface(&controller, &[LITTLE_V1.into()]),
            Some(NegotiatedSurface {
                layout_id: LITTLE_V1.into(),
                quality: SurfaceQuality::CertifiedCompatibility,
            })
        );
    }

    #[test]
    fn prefers_the_explicit_native_layout() {
        let controller = medium_controller(true);
        assert_eq!(
            negotiate_surface(&controller, &[LITTLE_V1.into(), "medium@1".into()]),
            Some(NegotiatedSurface {
                layout_id: "medium@1".into(),
                quality: SurfaceQuality::Native,
            })
        );
    }
}
