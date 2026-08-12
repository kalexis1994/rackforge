use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rackforge_resource_api::{
    BindResourceRequest, BrowseGrantRequest, GrantedResourceEntry, ResourceBrowser, ResourceEntry,
    ResourceEntryKind, ResourceError, ResourceGrant, ResourceMount,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};

#[derive(Clone, Debug)]
pub struct NativeMount {
    pub name: String,
    pub root: PathBuf,
    pub read_only: bool,
}

#[derive(Clone, Debug)]
struct MountState {
    public: ResourceMount,
    root: PathBuf,
    root_entry_id: String,
}

#[derive(Clone, Debug)]
struct EntryState {
    mount_id: String,
    path: PathBuf,
    parent_id: Option<String>,
}

#[derive(Default)]
struct BrowserState {
    mounts: Vec<MountState>,
    entries: HashMap<String, EntryState>,
    grants: Vec<GrantRecord>,
}

pub struct NativeResourceBrowser {
    state: Mutex<BrowserState>,
    grants_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GrantRecord {
    grant_id: String,
    plugin_id: String,
    resource_id: String,
    path: PathBuf,
    display_name: String,
    kind: ResourceEntryKind,
}

impl NativeResourceBrowser {
    pub fn new(mounts: impl IntoIterator<Item = NativeMount>) -> Result<Self, ResourceError> {
        Self::new_inner(mounts, None)
    }

    pub fn new_persistent(
        mounts: impl IntoIterator<Item = NativeMount>,
        grants_path: PathBuf,
    ) -> Result<Self, ResourceError> {
        Self::new_inner(mounts, Some(grants_path))
    }

    fn new_inner(
        mounts: impl IntoIterator<Item = NativeMount>,
        grants_path: Option<PathBuf>,
    ) -> Result<Self, ResourceError> {
        let mut state = BrowserState::default();
        if let Some(path) = grants_path.as_deref()
            && path.is_file()
        {
            let bytes = fs::read(path).map_err(backend)?;
            state.grants = serde_json::from_slice(&bytes)
                .map_err(|error| ResourceError::Backend(error.to_string()))?;
        }
        for source in mounts {
            let root = fs::canonicalize(&source.root).map_err(backend)?;
            if !root.is_dir() || state.mounts.iter().any(|mount| mount.root == root) {
                continue;
            }
            let mount_id = random_handle()?;
            let root_entry_id = random_handle()?;
            state.entries.insert(
                root_entry_id.clone(),
                EntryState {
                    mount_id: mount_id.clone(),
                    path: root.clone(),
                    parent_id: None,
                },
            );
            state.mounts.push(MountState {
                public: ResourceMount {
                    id: mount_id,
                    name: source.name,
                    read_only: source.read_only,
                },
                root,
                root_entry_id,
            });
        }
        Ok(Self {
            state: Mutex::new(state),
            grants_path,
        })
    }

    pub fn platform_defaults() -> Result<Self, ResourceError> {
        Self::new(default_mounts())
    }

    pub fn platform_defaults_persistent(grants_path: PathBuf) -> Result<Self, ResourceError> {
        Self::new_persistent(default_mounts(), grants_path)
    }

    fn entry_from_state(
        entry_id: String,
        entry: &EntryState,
        root: &Path,
    ) -> Result<ResourceEntry, ResourceError> {
        let metadata = fs::metadata(&entry.path).map_err(backend)?;
        let kind = if metadata.is_dir() {
            ResourceEntryKind::Directory
        } else if metadata.is_file() {
            ResourceEntryKind::File
        } else {
            return Err(ResourceError::Unreadable);
        };
        let name = if entry.path == root {
            entry.path.display().to_string()
        } else {
            entry
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .ok_or(ResourceError::Unreadable)?
        };
        let modified_unix_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| u64::try_from(duration.as_millis()).ok());
        Ok(ResourceEntry {
            id: entry_id,
            mount_id: entry.mount_id.clone(),
            parent_id: entry.parent_id.clone(),
            name,
            kind,
            size: metadata.is_file().then_some(metadata.len()),
            modified_unix_ms,
            lazy: metadata.is_dir(),
            can_read: true,
        })
    }

    pub fn resolve_granted_file(
        &self,
        plugin_id: &str,
        grant_id: &str,
        entry_id: Option<&str>,
    ) -> Result<PathBuf, ResourceError> {
        let state = self.state.lock().map_err(|_| poisoned())?;
        let grant = state
            .grants
            .iter()
            .find(|grant| grant.grant_id == grant_id && grant.plugin_id == plugin_id)
            .ok_or(ResourceError::UnknownHandle)?;
        let grant_root = fs::canonicalize(&grant.path).map_err(backend)?;
        let path = match (grant.kind, entry_id) {
            (ResourceEntryKind::File, None) => grant_root.clone(),
            (ResourceEntryKind::File, Some(_)) => {
                return Err(ResourceError::InvalidRequest(
                    "file grants must not include an entry id".into(),
                ));
            }
            (ResourceEntryKind::Directory, Some(entry_id)) => {
                let entry = state
                    .entries
                    .get(entry_id)
                    .ok_or(ResourceError::UnknownHandle)?;
                fs::canonicalize(&entry.path).map_err(backend)?
            }
            (ResourceEntryKind::Directory, None) => {
                return Err(ResourceError::InvalidRequest(
                    "directory grants require a selected file entry".into(),
                ));
            }
        };
        if !path.starts_with(&grant_root) {
            return Err(ResourceError::OutsideMount);
        }
        if !path.is_file() {
            return Err(ResourceError::InvalidRequest(
                "selected resource entry is not a file".into(),
            ));
        }
        Ok(path)
    }
}

impl ResourceBrowser for NativeResourceBrowser {
    fn mounts(&self) -> Result<Vec<ResourceMount>, ResourceError> {
        let state = self.state.lock().map_err(|_| poisoned())?;
        Ok(state
            .mounts
            .iter()
            .map(|mount| mount.public.clone())
            .collect())
    }

    fn mount_root(&self, mount_id: &str) -> Result<ResourceEntry, ResourceError> {
        let state = self.state.lock().map_err(|_| poisoned())?;
        let mount = state
            .mounts
            .iter()
            .find(|mount| mount.public.id == mount_id)
            .ok_or(ResourceError::UnknownHandle)?;
        let entry = state
            .entries
            .get(&mount.root_entry_id)
            .ok_or(ResourceError::UnknownHandle)?;
        let mut public = Self::entry_from_state(mount.root_entry_id.clone(), entry, &mount.root)?;
        public.name = mount.public.name.clone();
        Ok(public)
    }

    fn entries(&self, parent_id: &str) -> Result<Vec<ResourceEntry>, ResourceError> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        let parent = state
            .entries
            .get(parent_id)
            .cloned()
            .ok_or(ResourceError::UnknownHandle)?;
        let mount = state
            .mounts
            .iter()
            .find(|mount| mount.public.id == parent.mount_id)
            .cloned()
            .ok_or(ResourceError::UnknownHandle)?;
        let parent_path = fs::canonicalize(&parent.path).map_err(backend)?;
        if !parent_path.starts_with(&mount.root) {
            return Err(ResourceError::OutsideMount);
        }
        if !parent_path.is_dir() {
            return Err(ResourceError::NotDirectory);
        }

        let mut found = Vec::new();
        for candidate in fs::read_dir(&parent_path).map_err(backend)? {
            let candidate = match candidate {
                Ok(candidate) => candidate,
                Err(_) => continue,
            };
            let file_type = match candidate.file_type() {
                Ok(file_type) if !file_type.is_symlink() => file_type,
                _ => continue,
            };
            if !file_type.is_dir() && !file_type.is_file() {
                continue;
            }
            let path = match fs::canonicalize(candidate.path()) {
                Ok(path) if path.starts_with(&mount.root) => path,
                _ => continue,
            };
            let existing = state.entries.iter().find_map(|(id, entry)| {
                (entry.mount_id == mount.public.id && entry.path == path).then(|| id.clone())
            });
            let id = existing.unwrap_or(random_handle()?);
            let entry = EntryState {
                mount_id: mount.public.id.clone(),
                path,
                parent_id: Some(parent_id.to_owned()),
            };
            let public = match Self::entry_from_state(id.clone(), &entry, &mount.root) {
                Ok(public) => public,
                Err(_) => continue,
            };
            state.entries.insert(id, entry);
            found.push(public);
        }
        found.sort_by(|left, right| {
            let left_directory = left.kind == ResourceEntryKind::Directory;
            let right_directory = right.kind == ResourceEntryKind::Directory;
            right_directory
                .cmp(&left_directory)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        Ok(found)
    }

    fn bind(&self, request: &BindResourceRequest) -> Result<ResourceGrant, ResourceError> {
        if request.plugin_id.trim().is_empty() || request.resource_id.trim().is_empty() {
            return Err(ResourceError::InvalidRequest(
                "plugin_id and resource_id are required".into(),
            ));
        }
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        let entry = state
            .entries
            .get(&request.entry_id)
            .cloned()
            .ok_or(ResourceError::UnknownHandle)?;
        let mount = state
            .mounts
            .iter()
            .find(|mount| mount.public.id == entry.mount_id)
            .cloned()
            .ok_or(ResourceError::UnknownHandle)?;
        let public = Self::entry_from_state(request.entry_id.clone(), &entry, &mount.root)?;
        if let Some(existing) = state.grants.iter().find(|grant| {
            grant.plugin_id == request.plugin_id
                && grant.resource_id == request.resource_id
                && grant.path == entry.path
        }) {
            return Ok(ResourceGrant {
                grant_id: existing.grant_id.clone(),
                resource_id: existing.resource_id.clone(),
                display_name: existing.display_name.clone(),
                kind: existing.kind,
            });
        }
        let grant = GrantRecord {
            grant_id: random_handle()?,
            plugin_id: request.plugin_id.clone(),
            resource_id: request.resource_id.clone(),
            path: entry.path.clone(),
            display_name: public.name,
            kind: public.kind,
        };
        let response = ResourceGrant {
            grant_id: grant.grant_id.clone(),
            resource_id: grant.resource_id.clone(),
            display_name: grant.display_name.clone(),
            kind: grant.kind,
        };
        state.grants.push(grant);
        if let Some(path) = self.grants_path.as_deref() {
            persist_grants(path, &state.grants)?;
        }
        Ok(response)
    }

    fn grants(&self, plugin_id: &str) -> Result<Vec<ResourceGrant>, ResourceError> {
        let state = self.state.lock().map_err(|_| poisoned())?;
        Ok(state
            .grants
            .iter()
            .filter(|grant| grant.plugin_id == plugin_id)
            .map(|grant| ResourceGrant {
                grant_id: grant.grant_id.clone(),
                resource_id: grant.resource_id.clone(),
                display_name: grant.display_name.clone(),
                kind: grant.kind,
            })
            .collect())
    }

    fn grant_entries(
        &self,
        request: &BrowseGrantRequest,
    ) -> Result<Vec<GrantedResourceEntry>, ResourceError> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        let grant = state
            .grants
            .iter()
            .find(|grant| {
                grant.grant_id == request.grant_id && grant.plugin_id == request.plugin_id
            })
            .cloned()
            .ok_or(ResourceError::UnknownHandle)?;
        if grant.kind != ResourceEntryKind::Directory {
            return Err(ResourceError::NotDirectory);
        }
        let grant_root = fs::canonicalize(&grant.path).map_err(backend)?;
        let (parent_path, public_parent_id) = match request.parent_id.as_deref() {
            Some(parent_id) => {
                let entry = state
                    .entries
                    .get(parent_id)
                    .ok_or(ResourceError::UnknownHandle)?;
                (
                    fs::canonicalize(&entry.path).map_err(backend)?,
                    Some(parent_id),
                )
            }
            None => (grant_root.clone(), None),
        };
        if !parent_path.starts_with(&grant_root) {
            return Err(ResourceError::OutsideMount);
        }
        if !parent_path.is_dir() {
            return Err(ResourceError::NotDirectory);
        }
        let mount_id = state
            .mounts
            .iter()
            .find(|mount| grant_root.starts_with(&mount.root))
            .map(|mount| mount.public.id.clone())
            .ok_or(ResourceError::OutsideMount)?;
        let mut found = Vec::new();
        for candidate in fs::read_dir(&parent_path).map_err(backend)? {
            let candidate = match candidate {
                Ok(candidate) => candidate,
                Err(_) => continue,
            };
            let file_type = match candidate.file_type() {
                Ok(file_type) if !file_type.is_symlink() => file_type,
                _ => continue,
            };
            if !file_type.is_dir() && !file_type.is_file() {
                continue;
            }
            let path = match fs::canonicalize(candidate.path()) {
                Ok(path) if path.starts_with(&grant_root) => path,
                _ => continue,
            };
            let id = state
                .entries
                .iter()
                .find_map(|(id, entry)| (entry.path == path).then(|| id.clone()))
                .unwrap_or(random_handle()?);
            let entry = EntryState {
                mount_id: mount_id.clone(),
                path,
                parent_id: public_parent_id.map(str::to_owned),
            };
            let mount_root = state
                .mounts
                .iter()
                .find(|mount| mount.public.id == mount_id)
                .map(|mount| mount.root.clone())
                .ok_or(ResourceError::OutsideMount)?;
            let public = match Self::entry_from_state(id.clone(), &entry, &mount_root) {
                Ok(public) => public,
                Err(_) => continue,
            };
            state.entries.insert(id.clone(), entry);
            found.push(GrantedResourceEntry {
                id,
                parent_id: public.parent_id,
                name: public.name,
                kind: public.kind,
                size: public.size,
                modified_unix_ms: public.modified_unix_ms,
                lazy: public.lazy,
                can_read: public.can_read,
            });
        }
        found.sort_by(|left, right| {
            let left_directory = left.kind == ResourceEntryKind::Directory;
            let right_directory = right.kind == ResourceEntryKind::Directory;
            right_directory
                .cmp(&left_directory)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
        Ok(found)
    }
}

fn persist_grants(path: &Path, grants: &[GrantRecord]) -> Result<(), ResourceError> {
    let parent = path.parent().ok_or_else(|| {
        ResourceError::Backend("resource grant store has no parent directory".into())
    })?;
    fs::create_dir_all(parent).map_err(backend)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(grants)
        .map_err(|error| ResourceError::Backend(error.to_string()))?;
    fs::write(&temporary, bytes).map_err(backend)?;
    if path.exists() {
        fs::remove_file(path).map_err(backend)?;
    }
    fs::rename(temporary, path).map_err(backend)
}

fn random_handle() -> Result<String, ResourceError> {
    let mut bytes = [0_u8; 18];
    getrandom::fill(&mut bytes).map_err(|error| ResourceError::Backend(error.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn backend(error: std::io::Error) -> ResourceError {
    ResourceError::Backend(error.to_string())
}

fn poisoned() -> ResourceError {
    ResourceError::Backend("resource browser lock poisoned".into())
}

fn default_mounts() -> Vec<NativeMount> {
    #[cfg(windows)]
    {
        (b'A'..=b'Z')
            .filter_map(|letter| {
                let root = PathBuf::from(format!("{}:\\", char::from(letter)));
                root.is_dir().then(|| NativeMount {
                    name: format!("{}:", char::from(letter)),
                    root,
                    read_only: true,
                })
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut mounts = Vec::new();
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
            && home.is_dir()
        {
            mounts.push(NativeMount {
                name: "Home".into(),
                root: home,
                read_only: true,
            });
        }
        for (name, path) in [
            ("Media", "/media"),
            ("Mounted storage", "/mnt"),
            ("Removable media", "/run/media"),
        ] {
            let root = PathBuf::from(path);
            if root.is_dir() {
                mounts.push(NativeMount {
                    name: name.into(),
                    root,
                    read_only: true,
                });
            }
        }
        mounts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_do_not_reveal_native_paths() {
        let root = std::env::temp_dir();
        let browser = NativeResourceBrowser::new([NativeMount {
            name: "Test".into(),
            root: root.clone(),
            read_only: true,
        }])
        .unwrap();
        let mount = browser.mounts().unwrap().remove(0);
        let entry = browser.mount_root(&mount.id).unwrap();
        assert!(!mount.id.contains(&root.display().to_string()));
        assert!(!entry.id.contains(&root.display().to_string()));
    }

    #[test]
    fn file_grants_resolve_the_selected_file_without_a_child_entry() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-resource-file-grant-test-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        fs::create_dir_all(&root).unwrap();
        let source = root.join("IC36.bin");
        fs::write(&source, [1, 2, 3, 4]).unwrap();
        let browser = NativeResourceBrowser::new([NativeMount {
            name: "Test".into(),
            root: root.clone(),
            read_only: true,
        }])
        .unwrap();
        let mount = browser.mounts().unwrap().remove(0);
        let mount_root = browser.mount_root(&mount.id).unwrap();
        let file = browser
            .entries(&mount_root.id)
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == "IC36.bin")
            .unwrap();
        let grant = browser
            .bind(&BindResourceRequest {
                plugin_id: "org.rackforge.test".into(),
                resource_id: "pcm".into(),
                entry_id: file.id,
            })
            .unwrap();
        assert_eq!(
            browser
                .resolve_granted_file("org.rackforge.test", &grant.grant_id, None)
                .unwrap(),
            fs::canonicalize(&source).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn grants_remain_stable_across_host_restarts() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-resource-grant-test-{}",
            std::process::id()
        ));
        if root.exists() {
            fs::remove_dir_all(&root).unwrap();
        }
        let library = root.join("My SoundFonts");
        fs::create_dir_all(library.join("Banks")).unwrap();
        let grants_path = root.join("state/grants.json");
        let mount = NativeMount {
            name: "Test".into(),
            root: root.clone(),
            read_only: true,
        };

        let first =
            NativeResourceBrowser::new_persistent([mount.clone()], grants_path.clone()).unwrap();
        let first_mount = first.mounts().unwrap().remove(0);
        let first_root = first.mount_root(&first_mount.id).unwrap();
        let first_library = first
            .entries(&first_root.id)
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == "My SoundFonts")
            .unwrap();
        let request = BindResourceRequest {
            plugin_id: "org.rackforge.test".into(),
            resource_id: "library".into(),
            entry_id: first_library.id,
        };
        let first_grant = first.bind(&request).unwrap();
        let visible = first
            .grant_entries(&BrowseGrantRequest {
                plugin_id: request.plugin_id.clone(),
                grant_id: first_grant.grant_id.clone(),
                parent_id: None,
            })
            .unwrap();
        assert!(visible.iter().any(|entry| entry.name == "Banks"));
        assert_eq!(
            first.grants(&request.plugin_id).unwrap(),
            vec![first_grant.clone()]
        );
        assert_eq!(
            first
                .grant_entries(&BrowseGrantRequest {
                    plugin_id: "org.rackforge.other".into(),
                    grant_id: first_grant.grant_id.clone(),
                    parent_id: None,
                })
                .unwrap_err(),
            ResourceError::UnknownHandle
        );
        drop(first);

        let second = NativeResourceBrowser::new_persistent([mount], grants_path).unwrap();
        let second_mount = second.mounts().unwrap().remove(0);
        let second_root = second.mount_root(&second_mount.id).unwrap();
        let second_library = second
            .entries(&second_root.id)
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == "My SoundFonts")
            .unwrap();
        let second_grant = second
            .bind(&BindResourceRequest {
                entry_id: second_library.id,
                ..request
            })
            .unwrap();
        assert_eq!(first_grant.grant_id, second_grant.grant_id);

        fs::remove_dir_all(root).unwrap();
    }
}
