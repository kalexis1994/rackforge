//! The player's controller maps on disk: one JSON document per controller,
//! `<data-root>/controller-maps/<controller-id>.json`, the same map an
//! `.rfmap` file carries.
//!
//! A map belongs to the player, so it lives with their data rather than in
//! the controller package, which RackForge replaces on update. Writes go to
//! a temporary file renamed into place, so a map is never half written.
//!
//! RackForge ships a map for the controllers it bundles, made for its own
//! instruments. It is offered once, into the player's store, and kept up to
//! date only while the player leaves it as it came: the copy last offered
//! sits in `controller-maps/factory/`, and a stored map that still equals it
//! was never touched.

use anyhow::{Context, Result, bail};
use rackforge_midi_api::ControlTakeover;
use rackforge_midi_api::controller_map::{ControllerMap, RfMapFile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const CONTROLLER_MAP_DIRECTORY: &str = "controller-maps";
/// A map with every mapping it may hold stays far below this.
const MAX_MAP_BYTES: u64 = 4 * 1024 * 1024;
/// Where the factory map last offered for each controller is kept.
const FACTORY_DIRECTORY: &str = "factory";

/// The maps RackForge ships, as `.rfmap` documents.
const FACTORY_MAP_FILES: &[&str] = &[include_str!(
    "../../../hardware/controllers/arturia-keylab-essential-mk3/maps/rackforge-instruments.rfmap"
)];

/// The maps RackForge ships. One that does not read as a valid map is
/// reported and left out; a test keeps that from ever shipping.
pub fn factory_maps() -> Vec<ControllerMap> {
    FACTORY_MAP_FILES
        .iter()
        .filter_map(|text| {
            let parsed = serde_json::from_str::<RfMapFile>(text)
                .map_err(anyhow::Error::from)
                .and_then(|file| {
                    file.validate()
                        .map_err(|error| anyhow::anyhow!("invalid controller map: {error}"))?;
                    Ok(file.map)
                });
            parsed
                .map_err(|error| eprintln!("FACTORY_CONTROLLER_MAP_INVALID error={error:#}"))
                .ok()
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct ControllerMapStore {
    directory: Option<PathBuf>,
    /// The player's settings for every controller at once.
    settings: Option<PathBuf>,
}

/// What the player chose for every controller at once.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerSettings {
    schema_version: u32,
    #[serde(default)]
    takeover: ControlTakeover,
}

const CONTROLLER_SETTINGS_FILE: &str = "controller-settings.json";
const CONTROLLER_SETTINGS_SCHEMA_VERSION: u32 = 1;

impl ControllerMapStore {
    /// Without a data root the store keeps nothing on disk: every map is
    /// lost with the process, as the rest of such a session is.
    pub fn new(data_root: Option<&Path>) -> Self {
        Self {
            directory: data_root.map(|root| root.join(CONTROLLER_MAP_DIRECTORY)),
            settings: data_root.map(|root| root.join(CONTROLLER_SETTINGS_FILE)),
        }
    }

    /// How knobs and faders take a parameter over. A file that cannot be
    /// read is reported and taken as the default, pickup: a setting must
    /// never keep the controllers from working.
    pub fn takeover(&self) -> ControlTakeover {
        let Some(path) = &self.settings else {
            return ControlTakeover::default();
        };
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<ControllerSettings>(&bytes)
                .map(|settings| settings.takeover)
                .unwrap_or_else(|error| {
                    eprintln!("CONTROLLER_SETTINGS_SKIPPED path={path:?} error={error}");
                    ControlTakeover::default()
                }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ControlTakeover::default()
            }
            Err(error) => {
                eprintln!("CONTROLLER_SETTINGS_SKIPPED path={path:?} error={error}");
                ControlTakeover::default()
            }
        }
    }

    pub fn set_takeover(&self, takeover: ControlTakeover) -> Result<()> {
        let Some(path) = &self.settings else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let settings = ControllerSettings {
            schema_version: CONTROLLER_SETTINGS_SCHEMA_VERSION,
            takeover,
        };
        let temporary = path.with_extension(format!("json.new-{}", std::process::id()));
        fs::write(&temporary, serde_json::to_vec_pretty(&settings)?)
            .with_context(|| format!("writing {}", temporary.display()))?;
        fs::rename(&temporary, path).with_context(|| {
            let _ = fs::remove_file(&temporary);
            format!("replacing {}", path.display())
        })?;
        Ok(())
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
        write_map(&directory.join(file_name(&map.controller_id)?), map)
    }

    /// Offers every factory map, and names the controllers whose stored map
    /// it wrote. Run before `load_all`.
    pub fn seed_factory_maps(&self) -> Result<Vec<String>> {
        let mut seeded = Vec::new();
        for map in factory_maps() {
            if self.seed(&map)? {
                seeded.push(map.controller_id);
            }
        }
        Ok(seeded)
    }

    /// Writes a factory map where the player has no map and was never
    /// offered one, and over a map they kept exactly as it was last offered,
    /// so an update to it reaches them. A map of their own -- made before,
    /// edited since, or removed -- stays as they left it.
    fn seed(&self, factory: &ControllerMap) -> Result<bool> {
        let Some(directory) = &self.directory else {
            return Ok(false);
        };
        let name = file_name(&factory.controller_id)?;
        let offered_directory = directory.join(FACTORY_DIRECTORY);
        ensure_real_directory(directory)?;
        ensure_real_directory(&offered_directory)?;
        let offered_path = offered_directory.join(&name);
        // A damaged record is as good as none: the player's map still
        // decides, and only a missing one is written.
        let offered = match stored_map(&offered_path) {
            Stored::Map(map) => Some(map),
            Stored::Missing | Stored::Unreadable => None,
        };
        if offered.as_ref() == Some(factory) {
            return Ok(false);
        }
        let path = directory.join(&name);
        let write = match (offered, stored_map(&path)) {
            (None, Stored::Missing) => true,
            (Some(previous), Stored::Map(current)) => current == previous,
            _ => false,
        };
        if write {
            write_map(&path, factory)?;
        }
        write_map(&offered_path, factory)?;
        Ok(write)
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

fn write_map(path: &Path, map: &ControllerMap) -> Result<()> {
    let temporary = path.with_extension(format!("json.new-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(map)?;
    fs::write(&temporary, bytes).with_context(|| format!("writing {}", temporary.display()))?;
    fs::rename(&temporary, path).with_context(|| {
        let _ = fs::remove_file(&temporary);
        format!("replacing {}", path.display())
    })?;
    Ok(())
}

enum Stored {
    Missing,
    Map(ControllerMap),
    Unreadable,
}

fn stored_map(path: &Path) -> Stored {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Stored::Missing,
        _ => read_map(path).map_or(Stored::Unreadable, Stored::Map),
    }
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
    fn rackforge_ships_a_valid_keylab_map() {
        let maps = factory_maps();
        assert_eq!(
            maps.len(),
            FACTORY_MAP_FILES.len(),
            "every factory map reads"
        );
        let keylab = maps
            .iter()
            .find(|map| map.controller_id == "org.rackforge.arturia-keylab-essential-mk3")
            .expect("the KeyLab map ships");
        for plugin in [
            "org.rackforge.concert-grand",
            "org.rackforge.organ",
            "org.rackforge.rf-106",
            "org.rackforge.rf-5",
            "org.rackforge.rf7",
            "org.rackforge.rftines",
        ] {
            assert!(keylab.plugin(plugin).is_some(), "{plugin} is mapped");
        }
    }

    /// A range's end, written out by a plugin in single precision, has to
    /// read back as that very number: one bit past it, the plugin refuses the
    /// value, and a restored session holding it once kept the engine from
    /// starting.
    #[test]
    fn a_number_reads_back_as_the_value_written() {
        let noise_floor = f64::from(-24.082_401_f32);
        let written = serde_json::to_string(&noise_floor).unwrap();
        assert_eq!(written, "-24.082401275634766");
        let read: f64 = serde_json::from_str(&written).unwrap();
        assert_eq!(read.to_bits(), noise_floor.to_bits());
        let grand = factory_maps()
            .into_iter()
            .flat_map(|map| map.plugins)
            .find(|plugin| plugin.plugin_id == "org.rackforge.concert-grand")
            .unwrap();
        let floors = grand
            .mappings
            .iter()
            .flat_map(|mapping| mapping.mode.values())
            .filter(|value| *value < -24.0)
            .collect::<Vec<_>>();
        assert!(!floors.is_empty());
        assert!(
            floors
                .iter()
                .all(|value| value.to_bits() == noise_floor.to_bits())
        );
    }

    #[test]
    fn a_factory_map_is_offered_once_and_updated_while_untouched() {
        let root = root("factory");
        let store = ControllerMapStore::new(Some(&root.0));
        let first = leslie_map();
        assert!(store.seed(&first).unwrap(), "no map yet: offered");
        assert_eq!(store.load_all().unwrap()["user.oxygen-49"], first);
        assert!(!store.seed(&first).unwrap(), "offered already");

        // Kept as it came, a new version replaces it.
        let mut second = leslie_map();
        second.plugins[0].plugin_name = "RF-Organ 2".into();
        assert!(store.seed(&second).unwrap());
        assert_eq!(store.load_all().unwrap()["user.oxygen-49"], second);

        // Edited, it stays the player's.
        let mut edited = second.clone();
        edited.plugins[0].mappings[0].parameter_id = "drive".into();
        store.save(&edited).unwrap();
        let mut third = leslie_map();
        third.plugins[0].plugin_name = "RF-Organ 3".into();
        assert!(!store.seed(&third).unwrap());
        assert_eq!(store.load_all().unwrap()["user.oxygen-49"], edited);

        // Removed, it does not come back.
        store.remove("user.oxygen-49").unwrap();
        let mut fourth = leslie_map();
        fourth.plugins[0].plugin_name = "RF-Organ 4".into();
        assert!(!store.seed(&fourth).unwrap());
        assert!(store.load_all().unwrap().is_empty());
    }

    #[test]
    fn a_map_made_before_the_factory_one_stays() {
        let root = root("factory-own");
        let store = ControllerMapStore::new(Some(&root.0));
        let mut own = leslie_map();
        own.plugins[0].mappings[0].parameter_id = "drive".into();
        store.save(&own).unwrap();
        assert!(!store.seed(&leslie_map()).unwrap());
        assert_eq!(store.load_all().unwrap()["user.oxygen-49"], own);
    }

    #[test]
    fn the_takeover_setting_is_kept_and_defaults_to_pickup() {
        let root = root("takeover");
        let store = ControllerMapStore::new(Some(&root.0));
        assert_eq!(store.takeover(), ControlTakeover::Pickup);
        store.set_takeover(ControlTakeover::Scale).unwrap();
        let reopened = ControllerMapStore::new(Some(&root.0));
        assert_eq!(reopened.takeover(), ControlTakeover::Scale);
        // A damaged file costs the setting, never the controllers.
        fs::write(root.0.join(CONTROLLER_SETTINGS_FILE), b"{").unwrap();
        assert_eq!(reopened.takeover(), ControlTakeover::Pickup);
        // The settings file is not taken for a map.
        store.set_takeover(ControlTakeover::Jump).unwrap();
        store.save(&leslie_map()).unwrap();
        assert_eq!(store.load_all().unwrap().len(), 1);
    }

    #[test]
    fn an_export_is_named_after_the_controller() {
        let (name, file) = export_rfmap(&leslie_map(), "RackForge test", 7);
        assert_eq!(name, "Oxygen 49.rfmap");
        file.validate().unwrap();
    }
}
