//! The declarative controller packages RackForge ships.
//!
//! Each one lives in `hardware/controllers/catalog/<family>/<model>/` beside
//! the `SOURCES.md` that says where every value came from: the
//! manufacturer's documentation, never a guess. They are compiled in, so
//! every host -- the appliance's controller host, the desktop, Android --
//! installs the same set, as RackForge's own, on start.
//!
//! A package only names a controller's controls and what they mean by
//! default; it has no driver and cannot own a screen, so it is safe to have
//! installed for hardware the player does not own: it attaches only when a
//! MIDI port it matches appears.

use rackforge_controller_package::{
    CONTROLLER_MANIFEST_FILE, ControllerPackageManifest, PackageStore, PackageTrust,
    stamp_bundled_manifest,
};
use std::fs;
use std::path::Path;

/// One shipped package: where it lives in the catalog, and its manifest.
pub struct BundledController {
    pub path: &'static str,
    pub manifest: &'static str,
}

macro_rules! bundled {
    ($($path:literal),* $(,)?) => {
        &[$(BundledController {
            path: $path,
            manifest: include_str!(concat!(
                "../../../hardware/controllers/catalog/",
                $path,
                "/rackforge-controller.toml"
            )),
        }),*]
    };
}

/// Every package the catalog ships.
pub const BUNDLED: &[BundledController] = bundled![
    "novation-launchkey-mk3/25",
    "novation-launchkey-mk3/37",
    "novation-launchkey-mk3/49",
    "novation-launchkey-mk3/61",
    "novation-launchkey-mk3/88",
    "novation-launchkey-mini-mk3",
    "novation-launchkey-mk4/25",
    "novation-launchkey-mk4/37",
    "novation-launchkey-mk4/49",
    "novation-launchkey-mk4/61",
    "novation-launchkey-mini-mk4/25",
    "novation-launchkey-mini-mk4/37",
    "novation-launch-control-xl",
    "novation-launch-control-xl-3",
    "novation-flkey/mini",
    "novation-flkey/37",
    "novation-flkey/49",
    "novation-flkey/61",
    "novation-flkey-2/mini-25",
    "novation-flkey-2/37",
    "novation-flkey-2/49",
    "novation-flkey-2/61",
];

/// What installing the catalog did with one package.
#[derive(Debug)]
pub enum BundledInstall {
    Installed { id: String, version: String },
    Current { id: String },
    Failed { path: &'static str, error: String },
}

/// Installs every catalog package into the controller store at `root` as
/// RackForge's own, skipping the ones already installed at the same
/// version. A package that fails costs that package only.
///
/// The version each is installed under carries a digest of its manifest
/// (`+bundled.<digest>`), so a corrected manifest reaches players even when
/// its written version did not change.
pub fn install_bundled(root: &Path) -> Vec<BundledInstall> {
    let store = PackageStore::new(root);
    let installed = store.list().unwrap_or_default();
    let staging_root = root.join("staging").join("catalog");
    let results = BUNDLED
        .iter()
        .map(|bundled| {
            install_one(bundled, &store, &installed, &staging_root).unwrap_or_else(|error| {
                BundledInstall::Failed {
                    path: bundled.path,
                    error,
                }
            })
        })
        .collect();
    let _ = fs::remove_dir_all(&staging_root);
    results
}

fn install_one(
    bundled: &BundledController,
    store: &PackageStore,
    installed: &[rackforge_controller_package::InstalledController],
    staging_root: &Path,
) -> Result<BundledInstall, String> {
    let manifest_text =
        stamp_bundled_manifest(bundled.manifest, &[]).map_err(|error| error.to_string())?;
    let manifest: ControllerPackageManifest =
        toml::from_str(&manifest_text).map_err(|error| error.to_string())?;
    if installed.iter().any(|controller| {
        controller.record.id == manifest.id && controller.record.version == manifest.version
    }) {
        return Ok(BundledInstall::Current { id: manifest.id });
    }
    let staging = staging_root.join(&manifest.id);
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    fs::write(staging.join(CONTROLLER_MANIFEST_FILE), &manifest_text)
        .map_err(|error| error.to_string())?;
    let result = store
        .install_directory(&staging, PackageTrust::Official)
        .map_err(|error| error.to_string())?;
    Ok(BundledInstall::Installed {
        id: result.record.id,
        version: result.record.version,
    })
}

/// Installs the catalog and says what it did, one line per change, for
/// hosts that log to standard output.
pub fn install_bundled_and_report(root: &Path) {
    for result in install_bundled(root) {
        match result {
            BundledInstall::Installed { id, version } => {
                println!("CATALOG_CONTROLLER_INSTALLED id={id} version={version}");
            }
            BundledInstall::Current { .. } => {}
            BundledInstall::Failed { path, error } => {
                eprintln!("CATALOG_CONTROLLER_FAILED path={path} error={error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_controller_package::{DriverRuntimeKind, IdentityReply};
    use std::path::PathBuf;

    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp_root(name: &str) -> TempRoot {
        let path = std::env::temp_dir().join(format!(
            "rackforge-controller-catalog-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        TempRoot(path)
    }

    fn installed_store(name: &str) -> (TempRoot, PackageStore) {
        let root = temp_root(name);
        for result in install_bundled(&root.0) {
            if let BundledInstall::Failed { path, error } = result {
                panic!("{path}: {error}");
            }
        }
        let store = PackageStore::new(&root.0);
        (root, store)
    }

    fn reply(hex: &str) -> IdentityReply {
        let bytes: Vec<u8> = hex
            .split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect();
        IdentityReply::parse(&bytes).expect("a well-formed Identity Reply")
    }

    fn resolved(
        store: &PackageStore,
        port: &str,
        identity: Option<&IdentityReply>,
    ) -> Option<String> {
        store
            .resolve_identified_input(port, identity)
            .unwrap_or_else(|error| panic!("{port}: {error}"))
            .map(|binding| binding.controller_id)
    }

    #[test]
    fn every_package_is_valid_declarative_and_unique() {
        let mut ids = std::collections::BTreeSet::new();
        for bundled in BUNDLED {
            let stamped = stamp_bundled_manifest(bundled.manifest, &[])
                .unwrap_or_else(|error| panic!("{}: {error}", bundled.path));
            let manifest: ControllerPackageManifest = toml::from_str(&stamped).unwrap();
            assert_eq!(
                manifest.runtime.kind,
                DriverRuntimeKind::DeclarativeV1,
                "{}: the catalog ships no drivers",
                bundled.path
            );
            assert!(
                manifest.id.starts_with("org.rackforge."),
                "{}: RackForge's own id",
                bundled.path
            );
            assert!(ids.insert(manifest.id.clone()), "{} twice", manifest.id);
        }
    }

    #[test]
    fn installing_twice_changes_nothing_the_second_time() {
        let (root, _store) = installed_store("idempotent");
        for result in install_bundled(&root.0) {
            assert!(
                matches!(result, BundledInstall::Current { .. }),
                "{result:?}"
            );
        }
    }

    /// Every size answers with its documented Identity Reply
    /// (Programmer's Reference v1 p4) and is told apart by it; the DAW
    /// interface and the FLkey are never claimed, and the Launchkey Mini
    /// goes to its own package.
    #[test]
    fn a_launchkey_mk3_is_known_by_its_identity_reply() {
        let (_root, store) = installed_store("launchkey-mk3");
        for (size, dev_type) in [
            ("25", "34"),
            ("37", "35"),
            ("49", "36"),
            ("61", "37"),
            ("88", "40"),
        ] {
            let identity = reply(&format!(
                "F0 7E 00 06 02 00 20 29 {dev_type} 01 00 00 01 02 03 04 F7"
            ));
            let expected = format!("org.rackforge.novation-launchkey-mk3-{size}");
            // The port names below are how the systems are known to name the
            // two interfaces; Novation documents the interfaces, not these
            // names (see SOURCES.md). The matcher must hold for all of them.
            for port in [
                format!("Launchkey MK3 {size}"),
                format!("Launchkey MK3 {size}:Launchkey MK3 {size} LKMK3 MIDI Out 24:0"),
                format!("Launchkey MK3 {size} · Launchkey MK3 {size} LKMK3 MIDI Out"),
            ] {
                assert_eq!(
                    resolved(&store, &port, Some(&identity)).as_deref(),
                    Some(expected.as_str()),
                    "{port}"
                );
            }
            for port in [
                format!("MIDIIN2 (Launchkey MK3 {size})"),
                format!("Launchkey MK3 {size}:Launchkey MK3 {size} LKMK3 DAW Out 24:1"),
            ] {
                assert_eq!(resolved(&store, &port, Some(&identity)), None, "{port}");
            }
        }
        assert_eq!(
            resolved(
                &store,
                "KL Essential 61 mk3:KL Essential 61 mk3 MIDI 28:0",
                None
            ),
            None
        );
        // An FLkey never reaches a Launchkey MK3 package, whatever else
        // claims it.
        let flkey = resolved(&store, "FLkey 49 MK3", None);
        assert!(
            !flkey
                .as_deref()
                .is_some_and(|id| id.contains("launchkey-mk3")),
            "{flkey:?}"
        );
        for port in [
            "Launchkey Mini MK3 MIDI",
            "Launchkey Mini MK3:Launchkey Mini MK3 MIDI 1 20:0",
        ] {
            assert_eq!(
                resolved(&store, port, None).as_deref(),
                Some("org.rackforge.novation-launchkey-mini-mk3"),
                "{port}: the Mini, not a Launchkey MK3"
            );
        }
    }

    /// The Mini documents no Identity Reply, so its port name alone must
    /// single it out: the MIDI interface on every system, never the DAW
    /// port (User Guide 1.1 p11), and never another Mini generation.
    #[test]
    fn a_launchkey_mini_mk3_is_known_by_its_port_name() {
        let (_root, store) = installed_store("launchkey-mini-mk3");
        let mini = "org.rackforge.novation-launchkey-mini-mk3";
        for port in [
            "Launchkey Mini MK3",
            "Launchkey Mini MK3 MIDI Port",
            "Launchkey Mini MK3:Launchkey Mini MK3 MIDI 1 20:0",
            "Launchkey Mini MK3:Launchkey Mini MK3 MIDI 20:0",
            "Launchkey Mini MK3 · Launchkey Mini MK3 MIDI Port",
        ] {
            assert_eq!(
                resolved(&store, port, None).as_deref(),
                Some(mini),
                "{port}"
            );
        }
        for port in [
            "Launchkey Mini MK3 DAW Port",
            "Launchkey Mini MK3 (DAW Port)",
            "Launchkey Mini MIDI IN2",
            "MIDIIN2 (Launchkey Mini MK3)",
            "Launchkey Mini MK3:Launchkey Mini MK3 MIDI 2 20:1",
            "Launchkey Mini",
            "MIDIIN2 (Launchkey Mini)",
        ] {
            assert_eq!(resolved(&store, port, None), None, "{port}");
        }
        for port in [
            "Launchkey Mini MK4 25 MIDI",
            "Launchkey Mini MK4 37:Launchkey Mini MK4 37 MIDI 1 20:0",
        ] {
            assert_ne!(
                resolved(&store, port, None).as_deref(),
                Some(mini),
                "{port}: a Mini MK4"
            );
        }
    }

    /// The MK4 documents no Identity Reply, but its port names carry the
    /// size ("Launchkey MK4 61 MIDI" on Windows, "Launchkey MK4 49 MIDI Out"
    /// on a Mac, in Novation's FL Studio setup article). Each size claims its
    /// own MIDI interface; the DAW interface and the Minis stay out, and a
    /// name without the size is not guessed.
    #[test]
    fn a_launchkey_mk4_is_known_by_the_size_in_its_port_name() {
        let (_root, store) = installed_store("launchkey-mk4");
        for size in ["25", "37", "49", "61"] {
            let expected = format!("org.rackforge.novation-launchkey-mk4-{size}");
            for port in [
                format!("Launchkey MK4 {size} MIDI"),
                format!("Launchkey MK4 {size} MIDI Out"),
                format!("Launchkey MK4 {size}:Launchkey MK4 {size} MIDI 1 25:0"),
                format!("Launchkey MK4 {size}:Launchkey MK4 {size} MIDI 20:0"),
            ] {
                assert_eq!(
                    resolved(&store, &port, None).as_deref(),
                    Some(expected.as_str()),
                    "{port}"
                );
            }
            for port in [
                format!("MIDIIN2 (Launchkey MK4 {size} MIDI)"),
                format!("Launchkey MK4 {size} MIDI (Port 2)"),
                format!("Launchkey MK4 {size} DAW Out"),
                format!("Launchkey MK4 {size}:Launchkey MK4 {size} MIDI 2 25:1"),
            ] {
                assert_eq!(resolved(&store, &port, None), None, "{port}");
            }
            let mini = format!("Launchkey Mini MK4 {size} MIDI");
            assert_ne!(
                resolved(&store, &mini, None).as_deref(),
                Some(expected.as_str()),
                "{mini}: a Mini"
            );
        }
        for port in ["Launchkey MK4 MIDI", "LKMK4 MIDI"] {
            assert_eq!(resolved(&store, port, None), None, "{port}");
        }
    }

    /// The Mini MK4's port names as Novation's Ableton Live setup article
    /// shows them: Windows ("Launchkey Mini MK4 25 MIDI", its DAW port as
    /// "MIDIIN2 (...)" or "... (Port 2)") and macOS ("Launchkey Mini MK4 37
    /// (MIDI Out)", "(DAW Out)"). Each size claims its own MIDI interface,
    /// and the Launchkey MK4 packages never do.
    #[test]
    fn a_launchkey_mini_mk4_is_known_by_the_size_in_its_port_name() {
        let (_root, store) = installed_store("launchkey-mini-mk4");
        for size in ["25", "37"] {
            let expected = format!("org.rackforge.novation-launchkey-mini-mk4-{size}");
            for port in [
                format!("Launchkey Mini MK4 {size} MIDI"),
                format!("Launchkey Mini MK4 {size} (MIDI Out)"),
                format!("Launchkey Mini MK4 {size}:Launchkey Mini MK4 {size} MIDI 1 20:0"),
                format!("Launchkey Mini MK4 {size}:Launchkey Mini MK4 {size} MIDI 20:0"),
            ] {
                assert_eq!(
                    resolved(&store, &port, None).as_deref(),
                    Some(expected.as_str()),
                    "{port}"
                );
            }
            for port in [
                format!("MIDIIN2 (Launchkey Mini MK4 {size} MIDI)"),
                format!("Launchkey Mini MK4 {size} MIDI (Port 2)"),
                format!("Launchkey Mini MK4 {size} (DAW Out)"),
                format!("Launchkey Mini MK4 {size}:Launchkey Mini MK4 {size} MIDI 2 20:1"),
            ] {
                assert_eq!(resolved(&store, &port, None), None, "{port}");
            }
        }
        for port in [
            "LKMK4 MIDI",
            "MIDIIN2 (LKMK4 MIDI)",
            "Launchkey Mini MK4 MIDI",
        ] {
            assert_eq!(resolved(&store, port, None), None, "{port}");
        }
    }

    /// The Launch Control XL's one MIDI port is "Launch Control XL", with the
    /// device ID after it from ID 2 on (Programmer's Reference v2 p3). Its
    /// second interface and the plain Launch Control are never claimed, and
    /// the Launch Control XL 3 ("LCXL3 1 MIDI") goes to its own package.
    #[test]
    fn a_launch_control_xl_is_known_by_its_port_name() {
        let (_root, store) = installed_store("launch-control-xl");
        let xl = "org.rackforge.novation-launch-control-xl";
        for port in [
            "Launch Control XL",
            "Launch Control XL 2",
            "Launch Control XL:Launch Control XL MIDI 1 20:0",
            "Launch Control XL:Launch Control XL MIDI 20:0",
        ] {
            assert_eq!(resolved(&store, port, None).as_deref(), Some(xl), "{port}");
        }
        for port in [
            "MIDIIN2 (Launch Control XL)",
            "Launch Control XL (HUI)",
            "Launch Control XL:Launch Control XL MIDI 2 20:1",
            "Launch Control",
            "LCXL3 1 MIDI (Port 2)",
        ] {
            assert_eq!(resolved(&store, port, None), None, "{port}");
        }
        assert_ne!(
            resolved(&store, "LCXL3 1 MIDI", None).as_deref(),
            Some(xl),
            "an XL 3"
        );
    }

    /// The Launch Control XL 3's MIDI interface as Novation's Ableton Live
    /// setup article shows it: "LCXL3 1 MIDI" on Windows, "LCXL3 1 (MIDI
    /// Out)" on macOS, the number being the device ID. Its DAW interface,
    /// the Launch Control 3 ("LC3 1 MIDI") and the XL MK1/MK2 stay out.
    #[test]
    fn a_launch_control_xl_3_is_known_by_its_port_name() {
        let (_root, store) = installed_store("launch-control-xl-3");
        let xl3 = "org.rackforge.novation-launch-control-xl-3";
        for port in [
            "LCXL3 1 MIDI",
            "LCXL3 2 MIDI",
            "LCXL3 1 (MIDI Out)",
            "LCXL3 1:LCXL3 1 MIDI 1 20:0",
            "LCXL3 1:LCXL3 1 MIDI 20:0",
        ] {
            assert_eq!(resolved(&store, port, None).as_deref(), Some(xl3), "{port}");
        }
        for port in [
            "LCXL3 1 MIDI (Port 2)",
            "MIDIIN2 (LCXL3 1 MIDI)",
            "LCXL3 1 (DAW Out)",
            "LCXL3 1:LCXL3 1 MIDI 2 20:1",
            "LC3 1 MIDI",
        ] {
            assert_eq!(resolved(&store, port, None), None, "{port}");
        }
        assert_ne!(
            resolved(&store, "Launch Control XL", None).as_deref(),
            Some(xl3),
            "an XL MK2"
        );
    }

    /// The first FLkey generation's MIDI interface as its user guides show
    /// it ("FLkey Mini MIDI Out", "FLkey 61 FLkey MIDI Out"). Each model
    /// claims its own; the DAW interface, the FLkey 2 ("FLkey MK2 49 MIDI
    /// Out") and the Launchkeys stay out.
    #[test]
    fn an_flkey_is_known_by_the_model_in_its_port_name() {
        let (_root, store) = installed_store("flkey");
        for (model, port) in [
            ("mini", "FLkey Mini MIDI Out"),
            ("mini", "FLkey Mini"),
            ("37", "FLkey 37 FLkey MIDI Out"),
            ("37", "FLkey 37"),
            ("49", "FLkey 49 FLkey MIDI Out"),
            ("49", "FLkey 49:FLkey 49 MIDI 1 20:0"),
            ("61", "FLkey 61 FLkey MIDI Out"),
            ("61", "FLkey 61:FLkey 61 MIDI 20:0"),
        ] {
            let expected = format!("org.rackforge.novation-flkey-{model}");
            assert_eq!(
                resolved(&store, port, None).as_deref(),
                Some(expected.as_str()),
                "{port}"
            );
        }
        for port in [
            "FLkey Mini DAW Out",
            "FLkey 37 FLkey DAW Out",
            "MIDIIN2 (FLkey 49)",
            "FLkey 61 (Port 2)",
            "FLkey 61:FLkey 61 MIDI 2 20:1",
            "FLkey MIDI Out",
        ] {
            assert_eq!(resolved(&store, port, None), None, "{port}");
        }
    }

    /// The FLkey 2's MIDI interface as its user guides show it ("FLkey Mini
    /// MK2 25 MIDI Out", "FLkey MK2 61 MIDI Out"). Each model claims its own,
    /// and neither generation claims the other's.
    #[test]
    fn an_flkey_2_is_known_by_the_model_in_its_port_name() {
        let (_root, store) = installed_store("flkey-2");
        for (model, port) in [
            ("mini-25", "FLkey Mini MK2 25 MIDI Out"),
            ("37", "FLkey MK2 37 MIDI Out"),
            ("49", "FLkey MK2 49 MIDI Out"),
            ("61", "FLkey MK2 61 MIDI Out"),
            ("61", "FLkey MK2 61:FLkey MK2 61 MIDI 20:0"),
        ] {
            let expected = format!("org.rackforge.novation-flkey-2-{model}");
            assert_eq!(
                resolved(&store, port, None).as_deref(),
                Some(expected.as_str()),
                "{port}"
            );
        }
        for port in [
            "FLkey Mini MK2 25 DAW Out",
            "FLkey MK2 37 DAW Out",
            "MIDIIN2 (FLkey MK2 49 MIDI)",
            "FLkey MK2 61 MIDI (Port 2)",
            "FLkey MK2 61:FLkey MK2 61 MIDI 2 20:1",
        ] {
            assert_eq!(resolved(&store, port, None), None, "{port}");
        }
        for (port, first) in [
            ("FLkey 61 FLkey MIDI Out", "org.rackforge.novation-flkey-61"),
            ("FLkey Mini MIDI Out", "org.rackforge.novation-flkey-mini"),
        ] {
            assert_eq!(
                resolved(&store, port, None).as_deref(),
                Some(first),
                "{port}"
            );
        }
    }

    /// The Launch Control XL is put on User Template 1, the template its
    /// package describes, on its own port (Programmer's Reference v2 p8);
    /// the XL 3 on Mode 16, through its DAW port (PR 1.0 p16-17).
    #[test]
    fn a_launch_control_is_put_in_the_mode_its_package_describes() {
        let (_root, store) = installed_store("launch-control-setup");
        let xl = store
            .resolve_identified_input("Launch Control XL", None)
            .unwrap()
            .unwrap();
        assert_eq!(
            xl.on_connect,
            vec![vec![0xf0, 0x00, 0x20, 0x29, 0x02, 0x11, 0x77, 0x00, 0xf7]]
        );
        assert!(xl.setup_messages.is_empty());

        let xl3 = store
            .resolve_identified_input("LCXL3 1 MIDI", None)
            .unwrap()
            .unwrap();
        assert!(xl3.on_connect.is_empty());
        assert_eq!(
            xl3.setup_messages,
            vec![vec![0x9f, 0x0b, 0x7f], vec![0xb6, 0x1e, 0x1d]]
        );
        let setup = xl3.setup_output.as_ref().unwrap();
        let pick = |input: &str, outputs: &[&str]| {
            let outputs = outputs
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>();
            rackforge_controller_package::setup_output_port(setup, input, &outputs)
        };
        // Windows, either driver's name; macOS; two units by their IDs; and
        // never a DIN output.
        assert_eq!(
            pick(
                "LCXL3 1 MIDI",
                &[
                    "LCXL3 1 MIDI",
                    "MIDIOUT2 (LCXL3 1 MIDI)",
                    "MIDIOUT3 (LCXL3 1 MIDI)"
                ]
            )
            .as_deref(),
            Some("MIDIOUT2 (LCXL3 1 MIDI)")
        );
        assert_eq!(
            pick("LCXL3 1 MIDI", &["LCXL3 1 MIDI", "LCXL3 1 MIDI (Port 2)"]).as_deref(),
            Some("LCXL3 1 MIDI (Port 2)")
        );
        assert_eq!(
            pick(
                "LCXL3 2 (MIDI Out)",
                &[
                    "LCXL3 1 (DAW In)",
                    "LCXL3 2 (DAW In)",
                    "LCXL3 2 (To DIN Out)",
                    "LCXL3 2 (To DIN Out 2)"
                ]
            )
            .as_deref(),
            Some("LCXL3 2 (DAW In)")
        );
    }

    /// The Launchkey MK4's Play and Stop send MIDI Start and Stop (PR 3.0
    /// p7), and the Mini's Shift + Play sends Stop: both take the transport.
    #[test]
    fn a_launchkey_mk4_plays_and_stops_the_transport_with_midi_start_and_stop() {
        use rackforge_controller_api::{ButtonPhase, DeclarativeControllerInput, HostActionTarget};
        for path in [
            "novation-launchkey-mk4/25",
            "novation-launchkey-mk4/61",
            "novation-launchkey-mini-mk4/25",
            "novation-launchkey-mini-mk4/37",
        ] {
            let bundled = BUNDLED.iter().find(|bundled| bundled.path == path).unwrap();
            let stamped = stamp_bundled_manifest(bundled.manifest, &[]).unwrap();
            let manifest: ControllerPackageManifest = toml::from_str(&stamped).unwrap();
            let profile = manifest.profile();
            for (message, target) in [
                (0xfa, HostActionTarget::TransportPlay),
                (0xfc, HostActionTarget::TransportStop),
            ] {
                assert_eq!(
                    profile.declarative_input(&[message]),
                    Some(DeclarativeControllerInput::HostAction {
                        target,
                        phase: ButtonPhase::Press,
                    }),
                    "{path}"
                );
            }
            assert_eq!(
                profile.declarative_input(&[0xf8]),
                None,
                "{path}: the clock"
            );
        }
    }

    /// Five sizes share one port name pattern: without an Identity Reply
    /// the store cannot choose, and says so rather than picking one.
    #[test]
    fn a_launchkey_mk3_that_does_not_answer_is_not_guessed() {
        let (_root, store) = installed_store("launchkey-mk3-silent");
        assert!(
            store
                .resolve_identified_input("Launchkey MK3 49", None)
                .is_err()
        );
    }
}
