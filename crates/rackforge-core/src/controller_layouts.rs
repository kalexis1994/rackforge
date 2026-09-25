//! The maps RackForge offers each keyboard, made from the plugins' control
//! layouts and the slots the controller packages give their controls.
//!
//! A plugin lays itself out once, by slot (`control-1.3`, `switch-2.5`); a
//! controller package says once which of its controls fills each slot. Put
//! together, every installed plugin reaches every keyboard that names
//! slots, and a control of the same kind does the same thing everywhere.
//!
//! A plugin package carries its layout in `metadata/control-layout.json`.
//! RackForge ships the layouts of its own instruments too, for packages
//! released before they carried one; a package's own layout replaces
//! RackForge's.

use rackforge_controller_package::{
    ControllerPackageManifest, PackageStore, stamp_bundled_manifest,
};
use rackforge_midi_api::control_layout::{
    CONTROL_LAYOUT_FILE, ControlLayout, SlottedInput, derive_controller_map,
};
use rackforge_midi_api::controller_map::ControllerMap;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Far above any real layout: a guard against a corrupt or hostile file.
const MAX_LAYOUT_BYTES: u64 = 1024 * 1024;

/// The layouts of RackForge's own instruments.
const BUNDLED_LAYOUT_FILES: &[&str] = &[
    include_str!("../../../plugins/control-layouts/org.rackforge.organ.json"),
    include_str!("../../../plugins/control-layouts/org.rackforge.concert-grand.json"),
    include_str!("../../../plugins/control-layouts/org.rackforge.rf-106.json"),
    include_str!("../../../plugins/control-layouts/org.rackforge.rf-5.json"),
    include_str!("../../../plugins/control-layouts/org.rackforge.rf7.json"),
    include_str!("../../../plugins/control-layouts/org.rackforge.rftines.json"),
];

/// RackForge's layouts for its own instruments. One that does not read is
/// reported and left out; a test keeps that from ever shipping.
pub fn bundled_layouts() -> Vec<ControlLayout> {
    BUNDLED_LAYOUT_FILES
        .iter()
        .filter_map(|text| {
            parse_layout(text)
                .map_err(|error| eprintln!("BUNDLED_CONTROL_LAYOUT_INVALID error={error}"))
                .ok()
        })
        .collect()
}

/// The layout a plugin package carries, if it carries a valid one for
/// itself. A damaged one is reported and ignored: the plugin still plays.
pub fn package_layout(package_root: &Path, plugin_id: &str) -> Option<ControlLayout> {
    let path = package_root.join(CONTROL_LAYOUT_FILE);
    let metadata = fs::metadata(&path).ok()?;
    let layout = if metadata.len() > MAX_LAYOUT_BYTES {
        Err(format!("larger than {MAX_LAYOUT_BYTES} bytes"))
    } else {
        fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| parse_layout(&text))
            .and_then(|layout| {
                if layout.plugin_id == plugin_id {
                    Ok(layout)
                } else {
                    Err(format!("it lays out {:?}", layout.plugin_id))
                }
            })
    };
    layout
        .map_err(|error| {
            eprintln!("CONTROL_LAYOUT_SKIPPED plugin={plugin_id} path={path:?} error={error}")
        })
        .ok()
}

/// Every layout to offer: RackForge's, each replaced by the one its plugin's
/// installed package carries, then the layouts other plugins carry.
/// `packages` names each installed plugin and its package root.
pub fn control_layouts<'a>(
    packages: impl IntoIterator<Item = (&'a str, &'a Path)>,
) -> Vec<ControlLayout> {
    let mut layouts = bundled_layouts()
        .into_iter()
        .map(|layout| (layout.plugin_id.clone(), layout))
        .collect::<Vec<_>>();
    let mut others = BTreeMap::new();
    for (plugin_id, root) in packages {
        let Some(layout) = package_layout(root, plugin_id) else {
            continue;
        };
        match layouts.iter_mut().find(|(id, _)| id == plugin_id) {
            Some((_, bundled)) => *bundled = layout,
            None => {
                others.insert(plugin_id.to_owned(), layout);
            }
        }
    }
    layouts
        .into_iter()
        .map(|(_, layout)| layout)
        .chain(others.into_values())
        .collect()
}

/// A controller package that names slots, as a map is made from it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlottedController {
    pub id: String,
    pub name: String,
    pub inputs: Vec<SlottedInput>,
}

impl SlottedController {
    fn from_manifest(manifest: &ControllerPackageManifest) -> Option<Self> {
        let inputs = manifest.slotted_inputs();
        (!inputs.is_empty()).then(|| Self {
            id: manifest.id.clone(),
            name: manifest.display_name(),
            inputs,
        })
    }
}

/// RackForge's own controller packages beyond the catalog: known here so
/// they are offered their map even before a host installs them.
const RACKFORGE_CONTROLLERS: &[&str] = &[include_str!(
    "../../../hardware/controllers/arturia-keylab-essential-mk3/package/rackforge-controller.toml"
)];

/// Every controller that names slots: the packages installed at
/// `controllers_root`, and RackForge's own -- the catalog and the KeyLab --
/// where the store does not hold them yet, so a keyboard is offered its map
/// even before its package is installed.
pub fn slotted_controllers(controllers_root: Option<&Path>) -> Vec<SlottedController> {
    known_controller_packages(controllers_root)
        .iter()
        .filter_map(SlottedController::from_manifest)
        .collect()
}

/// Every controller package a host knows: those installed at
/// `controllers_root`, and RackForge's own -- the catalog and the KeyLab.
/// For RackForge's own, the copy this build carries wins: the installed one
/// was made from it, and may be an older build's until the host installs
/// again.
pub fn known_controller_packages(
    controllers_root: Option<&Path>,
) -> Vec<ControllerPackageManifest> {
    let mut packages = BTreeMap::new();
    if let Some(root) = controllers_root.filter(|root| root.exists()) {
        match PackageStore::new(root).list() {
            Ok(installed) => {
                for controller in installed {
                    let manifest = controller.package.manifest().clone();
                    packages.insert(manifest.id.clone(), manifest);
                }
            }
            Err(error) => eprintln!("CONTROLLER_PACKAGES_UNREADABLE root={root:?} error={error}"),
        }
    }
    let own = rackforge_controller_catalog::BUNDLED
        .iter()
        .map(|bundled| bundled.manifest)
        .chain(RACKFORGE_CONTROLLERS.iter().copied());
    for text in own {
        let manifest = stamp_bundled_manifest(text, &[])
            .ok()
            .and_then(|text| toml::from_str::<ControllerPackageManifest>(&text).ok());
        if let Some(manifest) = manifest {
            packages.insert(manifest.id.clone(), manifest);
        }
    }
    packages.into_values().collect()
}

/// The map each controller is offered: every layout on its slots. A
/// controller none of whose slots a layout fills is offered none.
pub fn factory_maps(
    controllers: &[SlottedController],
    layouts: &[ControlLayout],
) -> Vec<ControllerMap> {
    controllers
        .iter()
        .map(|controller| {
            derive_controller_map(
                &controller.id,
                &controller.name,
                &controller.inputs,
                layouts,
            )
        })
        .filter(|map| !map.is_empty() && map.validate().is_ok())
        .collect()
}

fn parse_layout(text: &str) -> Result<ControlLayout, String> {
    let layout: ControlLayout = serde_json::from_str(text).map_err(|error| error.to_string())?;
    layout.validate().map_err(|error| error.to_string())?;
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_midi_api::control_layout::ControlSlot;

    const KEYLAB_PACKAGE: &str = include_str!(
        "../../../hardware/controllers/arturia-keylab-essential-mk3/package/rackforge-controller.toml"
    );

    fn keylab() -> SlottedController {
        let manifest: ControllerPackageManifest = toml::from_str(KEYLAB_PACKAGE).unwrap();
        manifest.validate().unwrap();
        SlottedController::from_manifest(&manifest).unwrap()
    }

    #[test]
    fn rackforge_lays_out_its_six_instruments() {
        let layouts = bundled_layouts();
        assert_eq!(
            layouts.len(),
            BUNDLED_LAYOUT_FILES.len(),
            "every layout reads"
        );
        let ids = layouts
            .iter()
            .map(|layout| layout.plugin_id.as_str())
            .collect::<Vec<_>>();
        for plugin in [
            "org.rackforge.concert-grand",
            "org.rackforge.organ",
            "org.rackforge.rf-106",
            "org.rackforge.rf-5",
            "org.rackforge.rf7",
            "org.rackforge.rftines",
        ] {
            assert!(ids.contains(&plugin), "{plugin}");
        }
    }

    /// Every catalog keyboard and the KeyLab get a valid map of all six
    /// instruments, each control heard as its package declares it.
    #[test]
    fn every_slotted_controller_is_offered_a_valid_map() {
        let controllers = slotted_controllers(None);
        assert_eq!(
            controllers.len(),
            rackforge_controller_catalog::BUNDLED.len() + RACKFORGE_CONTROLLERS.len()
        );
        assert!(
            controllers
                .iter()
                .any(|controller| controller.id == keylab().id)
        );
        let maps = factory_maps(&controllers, &bundled_layouts());
        assert_eq!(maps.len(), controllers.len());
        for (map, controller) in maps.iter().zip(&controllers) {
            map.validate().unwrap();
            // A keyboard with a row of knobs or faders plays every
            // instrument; one with only a modulation wheel (a Keystation)
            // those that lay the wheel out.
            let has_row = controller
                .inputs
                .iter()
                .any(|input| matches!(input.slot, ControlSlot::Control { .. }));
            let least = if has_row { 5 } else { 1 };
            assert!(map.plugins.len() >= least, "{}", map.controller_id);
        }
    }

    /// The KeyLab's layout came from its own map: laid out again on its
    /// slots, it plays the organ as before.
    #[test]
    fn the_keylab_plays_the_organ_on_the_controls_it_always_did() {
        let map = factory_maps(&[keylab()], &bundled_layouts()).remove(0);
        let organ = map.plugin("org.rackforge.organ").unwrap();
        let on = |input: &str| {
            organ
                .mappings
                .iter()
                .find(|mapping| mapping.input.id == input)
                .map(|mapping| mapping.parameter_id.as_str())
        };
        assert_eq!(on("fader-1"), Some("drawbar-16"));
        assert_eq!(on("encoder-8"), Some("drawbar-1"));
        assert_eq!(on("pad-b8"), Some("rotary-mode"));
        assert_eq!(on("rewind"), Some("registration"));
        assert_eq!(on("undo"), Some("scanner-mode"));
        // Knob 9 and fader 9 keep the master pan and level.
        assert_eq!(on("encoder-9"), None);
        assert_eq!(on("fader-9"), None);
    }

    #[test]
    fn a_plugin_package_lays_itself_out() {
        let root = std::env::temp_dir().join(format!("rackforge-layout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("metadata")).unwrap();
        let mut organ = bundled_layouts()
            .into_iter()
            .find(|layout| layout.plugin_id == "org.rackforge.organ")
            .unwrap();
        organ.slots.truncate(1);
        fs::write(
            root.join(CONTROL_LAYOUT_FILE),
            serde_json::to_string(&organ).unwrap(),
        )
        .unwrap();
        let layouts = control_layouts([("org.rackforge.organ", root.as_path())]);
        let used = layouts
            .iter()
            .find(|layout| layout.plugin_id == "org.rackforge.organ")
            .unwrap();
        assert_eq!(used.slots.len(), 1, "the package's own layout wins");
        assert_eq!(layouts.len(), BUNDLED_LAYOUT_FILES.len());

        // A layout for another plugin is not taken.
        assert!(package_layout(&root, "org.rackforge.rf-106").is_none());
        let _ = fs::remove_dir_all(&root);
    }
}
