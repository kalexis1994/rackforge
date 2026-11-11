//! What a plugin store records beside each package it installs.
//!
//! The store keeps `packages/<id>/<version>/` and, for each, a record at
//! `records/<id>/<version>.json` saying where the package came from. The
//! installer writes it (`rackforge-repository`); a host reads it here, to
//! decide what an installed package is trusted with: a package the release
//! itself laid down may run its native build in place of its component, and
//! every other package runs sandboxed.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// The source of a package laid down from the release's own official set:
/// pinned by URL and checked against a SHA-256 the source tree carries, so
/// it is as trusted as the host that carries it.
pub const OFFICIAL_REPOSITORY_ID: &str = "official";
/// The source of a package a user installed from a file.
pub const LOCAL_REPOSITORY_ID: &str = "local";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationRecord {
    pub schema_version: u32,
    pub plugin_id: String,
    pub version: String,
    pub platform: String,
    pub repository_id: String,
    pub artifact_sha256: String,
}

impl InstallationRecord {
    /// Whether the release's official set laid this package down.
    pub fn is_official(&self) -> bool {
        self.repository_id == OFFICIAL_REPOSITORY_ID
    }

    /// The same installation from another source: everything but where it
    /// came from is equal, the bytes included.
    pub fn same_payload(&self, other: &Self) -> bool {
        Self {
            repository_id: String::new(),
            ..self.clone()
        } == Self {
            repository_id: String::new(),
            ..other.clone()
        }
    }
}

/// The store's record for the installed package at `package_root`, which a
/// store keeps at `<store>/packages/<id>/<version>`; `None` for a package
/// outside a store, or one whose record does not name it.
pub fn installation_record_for(
    package_root: &Path,
    plugin_id: &str,
    version: &str,
) -> Option<InstallationRecord> {
    let package_root = package_root.canonicalize().ok()?;
    let version_directory = package_root.file_name()?.to_str()?;
    let plugin_directory = package_root.parent()?;
    let packages = plugin_directory.parent()?;
    if version_directory != version
        || plugin_directory.file_name()?.to_str()? != plugin_id
        || packages.file_name()?.to_str()? != "packages"
    {
        return None;
    }
    let record_path = packages
        .parent()?
        .join("records")
        .join(plugin_id)
        .join(format!("{version}.json"));
    let record: InstallationRecord =
        serde_json::from_slice(&std::fs::read(record_path).ok()?).ok()?;
    (record.plugin_id == plugin_id && record.version == version).then_some(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(repository_id: &str) -> InstallationRecord {
        InstallationRecord {
            schema_version: 1,
            plugin_id: "org.example.synth".into(),
            version: "1.2.3".into(),
            platform: "wasm-v1".into(),
            repository_id: repository_id.into(),
            artifact_sha256: "ab".repeat(32),
        }
    }

    #[test]
    fn a_package_is_official_only_by_its_record_in_its_own_store() {
        let store =
            std::env::temp_dir().join(format!("rackforge-install-record-{}", std::process::id()));
        let package = store.join("packages/org.example.synth/1.2.3");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::create_dir_all(store.join("records/org.example.synth")).unwrap();
        let record_path = store.join("records/org.example.synth/1.2.3.json");

        // No record: a package outside a store, or not installed by one.
        assert_eq!(
            installation_record_for(&package, "org.example.synth", "1.2.3"),
            None
        );

        std::fs::write(
            &record_path,
            serde_json::to_vec(&record("official")).unwrap(),
        )
        .unwrap();
        let found = installation_record_for(&package, "org.example.synth", "1.2.3").unwrap();
        assert!(found.is_official());
        // A manifest that names another plugin or version is not this record's.
        assert_eq!(
            installation_record_for(&package, "org.example.other", "1.2.3"),
            None
        );
        assert_eq!(
            installation_record_for(&package, "org.example.synth", "1.2.4"),
            None
        );

        std::fs::write(&record_path, serde_json::to_vec(&record("local")).unwrap()).unwrap();
        assert!(
            !installation_record_for(&package, "org.example.synth", "1.2.3")
                .unwrap()
                .is_official()
        );
        // A record that does not parse is no record at all.
        std::fs::write(&record_path, b"{").unwrap();
        assert_eq!(
            installation_record_for(&package, "org.example.synth", "1.2.3"),
            None
        );

        assert!(record("official").same_payload(&record("local")));
        let mut other = record("local");
        other.artifact_sha256 = "cd".repeat(32);
        assert!(!record("official").same_payload(&other));
        std::fs::remove_dir_all(store).ok();
    }
}
