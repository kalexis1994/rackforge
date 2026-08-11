//! Host-owned resource browsing contracts.
//!
//! Plugins identify declared resources, while RackForge owns filesystem access,
//! platform permissions and browsing. Public handles are deliberately opaque:
//! a plugin Web surface never receives a native path or Android content URI.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceEntryKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceMount {
    pub id: String,
    pub name: String,
    pub read_only: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceEntry {
    pub id: String,
    pub mount_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: ResourceEntryKind,
    pub size: Option<u64>,
    pub modified_unix_ms: Option<u64>,
    pub lazy: bool,
    pub can_read: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BindResourceRequest {
    pub plugin_id: String,
    pub resource_id: String,
    pub entry_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceGrant {
    pub grant_id: String,
    pub resource_id: String,
    pub display_name: String,
    pub kind: ResourceEntryKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BrowseGrantRequest {
    pub plugin_id: String,
    pub grant_id: String,
    pub parent_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListGrantsRequest {
    pub plugin_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoadGrantedResourceRequest {
    pub plugin_id: String,
    pub instance_id: String,
    pub target_resource_id: String,
    pub grant_id: String,
    pub entry_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrantedResourceEntry {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub kind: ResourceEntryKind,
    pub size: Option<u64>,
    pub modified_unix_ms: Option<u64>,
    pub lazy: bool,
    pub can_read: bool,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ResourceError {
    #[error("resource handle was not found")]
    UnknownHandle,
    #[error("resource handle is outside its authorized mount")]
    OutsideMount,
    #[error("resource is not a directory")]
    NotDirectory,
    #[error("resource cannot be read")]
    Unreadable,
    #[error("resource request is invalid: {0}")]
    InvalidRequest(String),
    #[error("resource backend failed: {0}")]
    Backend(String),
}

pub trait ResourceBrowser: Send + Sync {
    fn mounts(&self) -> Result<Vec<ResourceMount>, ResourceError>;
    fn mount_root(&self, mount_id: &str) -> Result<ResourceEntry, ResourceError>;
    fn entries(&self, parent_id: &str) -> Result<Vec<ResourceEntry>, ResourceError>;
    fn bind(&self, request: &BindResourceRequest) -> Result<ResourceGrant, ResourceError>;
    fn grants(&self, plugin_id: &str) -> Result<Vec<ResourceGrant>, ResourceError>;
    fn grant_entries(
        &self,
        request: &BrowseGrantRequest,
    ) -> Result<Vec<GrantedResourceEntry>, ResourceError>;
}
