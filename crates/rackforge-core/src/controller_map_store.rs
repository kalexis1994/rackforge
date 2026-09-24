//! The player's controller maps on disk: one JSON document per controller,
//! `<data-root>/controller-maps/<controller-id>.json`, the same map an
//! `.rfmap` file carries.
//!
//! A map belongs to the player, so it lives with their data rather than in
//! the controller package, which RackForge replaces on update. Writes go to
//! a temporary file renamed into place, so a map is never half written.

use anyhow::{Context, Result, bail};
use rackforge_midi_api::controller_map::{ControllerMap, RfMapFile};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONTROLLER_MAP_DIRECTORY: &str = "controller-maps";
/// A map with every mapping it may hold stays far below this.
const MAX_MAP_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct ControllerMapStore {
    directory: Option<PathBuf>,
}

impl ControllerMapStore {
    /// Without a data root the store keeps nothing on disk: every map is
    /// lost with the process, as the rest of such a session is.
    pub fn new(data_root: Option<&Path>) -> Self {
        Self {
            directory: data_root.map(|root| root.join(CONTROLLER_MAP_DIRECTORY)),
        }
    }

    /// Every stored map, by controller id. A file that does not read as a
    /// valid map is reported and left where it is: one damaged map must not
    /// cost the player the others.
    pub fn load_all(&self) -> Result<BTreeMap<String, ControllerMap>> {
        let mut maps = BTreeMap::new();
        let Some(directory) = &self.directory else {
            return Ok(maps);
        };
        if !directory.exists() {
            return Ok(maps);
        }
        ensure_real_directory(directory)?;
        for entry in
            fs::read_dir(directory).with_context(|| format!("reading {}", directory.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            match read_map(&path) {
                Ok(map) => {
                    maps.insert(map.controller_id.clone(), map);
                }
                Err(error) => {
                    eprintln!("CONTROLLER_MAP_SKIPPED path={path:?} error={error:#}");
                }
            }
        }
        Ok(maps)
    }

    /// Stores a controller's whole map, replacing what was there. A map with
    /// no mapping left is removed rather than kept empty.
    pub fn save(&self, map: &ControllerMap) -> Result<()> {
        map.validate()
            .map_err(|error| anyhow::anyhow!("invalid controller map: {error}"))?;
        if map.is_empty() {
            return self.remove(&map.controller_id);
        }
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        ensure_real_directory(directory)?;
        let path = directory.join(file_name(&map.controller_id)?);
        let temporary = path.with_extension(format!("json.new-{}", std::process::id()));
        let bytes = serde_json::to_vec_pretty(map)?;
        fs::write(&temporary, bytes).with_context(|| format!("writing {}", temporary.display()))?;
        fs::rename(&temporary, &path).with_context(|| {
            let _ = fs::remove_file(&temporary);
            format!("replacing {}", path.display())
        })?;
        Ok(())
    }

    pub fn remove(&self, controller_id: &str) -> Result<()> {
        let Some(directory) = &self.directory else {
            return Ok(());
        };
        let path = directory.join(file_name(controller_id)?);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("removing {}", path.display())),
        }
    }
}

/// The `.rfmap` document for a map, and the name to save it under.
pub fn export_rfmap(
    map: &ControllerMap,
    exported_by: &str,
    exported_unix_ms: u64,
) -> (String, RfMapFile) {
    let stem = map
        .controller_name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || " -_".contains(character) {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let stem = stem.trim();
    let stem = if stem.is_empty() { "Controller" } else { stem };
    (
        format!("{stem}.rfmap"),
        RfMapFile::new(map.clone(), exported_by, exported_unix_ms),
    )
}

/// A controller package id names the file. Package ids are lowercase
/// identifiers already; anything else is refused rather than escaped, so a
/// map can never be written outside its directory.
fn file_name(controller_id: &str) -> Result<String> {
    let valid = !controller_id.is_empty()
        && controller_id.len() <= 128
        && !controller_id.starts_with('.')
        && !controller_id.contains("..")
        && controller_id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-_".contains(&byte)
        });
    if !valid {
        bail!("controller id {controller_id:?} cannot name a map file");
    }
    Ok(format!("{controller_id}.json"))
}

fn read_map(path: &Path) -> Result<ControllerMap> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("not a regular file");
    }
    if metadata.len() == 0 || metadata.len() > MAX_MAP_BYTES {
        bail!("invalid size {}", metadata.len());
    }
    let map: ControllerMap = serde_json::from_slice(&fs::read(path)?)?;
    map.validate()
        .map_err(|error| anyhow::anyhow!("invalid controller map: {error}"))?;
    if path.file_name().and_then(|name| name.to_str())
        != Some(file_name(&map.controller_id)?.as_str())
    {
        bail!("the map inside belongs to {:?}", map.controller_id);
    }
    Ok(map)
}

fn ensure_real_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "controller map path must be a real directory: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_midi_api::controller_map::{ControlMapping, MappedInput, PluginControlMap};
    use rackforge_midi_api::{
        LinkValue, ParameterLinkChannel, ParameterLinkId, ParameterLinkMessage, ParameterLinkMode,
    };

    struct Root(PathBuf);

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn root(name: &str) -> Root {
        let path = std::env::temp_dir().join(format!(
            "rackforge-controller-maps-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        Root(path)
    }

    fn leslie_map() -> ControllerMap {
        let mut map = ControllerMap::new("user.oxygen-49", "Oxygen 49");
        map.plugins.push(PluginControlMap {
            plugin_id: "org.rackforge.organ".into(),
            plugin_name: "RF-Organ".into(),
            mappings: vec![ControlMapping {
                id: ParameterLinkId::new("map.leslie").unwrap(),
                input: MappedInput {
                    id: "button-1".into(),
                    name: "Button 1".into(),
                    channel: ParameterLinkChannel::Omni,
                    message: ParameterLinkMessage::ControlChange { controller: 20 },
                },
                parameter_id: "leslie.speed".into(),
                mode: ParameterLinkMode::Toggle {
                    first: LinkValue::new(1.0).unwrap(),
                    second: LinkValue::new(2.0).unwrap(),
                },
                invert: false,
                pass_through: None,
            }],
        });
        map
    }

    #[test]
    fn a_map_is_stored_and_read_back() {
        let root = root("round-trip");
        let store = ControllerMapStore::new(Some(&root.0));
        assert!(store.load_all().unwrap().is_empty());
        store.save(&leslie_map()).unwrap();
        let maps = store.load_all().unwrap();
        assert_eq!(maps.get("user.oxygen-49"), Some(&leslie_map()));
    }

    #[test]
    fn an_emptied_map_is_removed() {
        let root = root("emptied");
        let store = ControllerMapStore::new(Some(&root.0));
        store.save(&leslie_map()).unwrap();
        let mut emptied = leslie_map();
        emptied.plugins[0].mappings.clear();
        store.save(&emptied).unwrap();
        assert!(store.load_all().unwrap().is_empty());
    }

    #[test]
    fn a_damaged_map_costs_nothing_else() {
        let root = root("damaged");
        let store = ControllerMapStore::new(Some(&root.0));
        store.save(&leslie_map()).unwrap();
        fs::write(
            root.0
                .join(CONTROLLER_MAP_DIRECTORY)
                .join("user.broken.json"),
            b"{",
        )
        .unwrap();
        assert_eq!(store.load_all().unwrap().len(), 1);
    }

    #[test]
    fn a_controller_id_cannot_leave_the_directory() {
        assert!(file_name("../escape").is_err());
        assert!(file_name("user/escape").is_err());
        assert!(file_name("User.Upper").is_err());
        assert_eq!(file_name("user.oxygen-49").unwrap(), "user.oxygen-49.json");
    }

    #[test]
    fn an_export_is_named_after_the_controller() {
        let (name, file) = export_rfmap(&leslie_map(), "RackForge test", 7);
        assert_eq!(name, "Oxygen 49.rfmap");
        file.validate().unwrap();
    }
}
