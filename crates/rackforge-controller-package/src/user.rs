//! Controllers a player makes: a keyboard RackForge has no package for,
//! described by moving its controls in the Controllers editor.
//!
//! The editor sends the controls it heard, named as the player named them;
//! this builds a declarative schema 2 package of them -- identity, inputs,
//! no meanings -- and installs it like any other. Every save is a new
//! version, since an installed version is never changed in place, and a
//! player's package never replaces one RackForge or a vendor published.

use crate::{
    CONTROLLER_MANIFEST_FILE, CONTROLLER_PACKAGE_SCHEMA_VERSION_2, ControllerInput,
    ControllerPackageManifest, DeviceMatcher, EndpointMatcher, EndpointRole, InstalledController,
    PackageError, PackageStore, PackageTrust, declarative_runtime, input_only_permissions,
};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::fs;

/// Every package a player makes has an id under this prefix.
pub const USER_CONTROLLER_PREFIX: &str = "user.";
/// The controller API a player's package asks for: what schema 2 needs.
const USER_CONTROLLER_API: &str = "^1.0";
/// Words that name a controller's other ports -- its DAW or Mackie port, a
/// MIDI thru -- which must not be taken for the one the player chose.
const AUXILIARY_PORT_WORDS: [&str; 7] =
    ["midiin", "midiout", "daw", "mcu", "hui", "dinthru", "thru"];

/// A controller as the editor describes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserControllerRequest {
    /// The package to save again; left out, a new one is made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_id: Option<String>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// The MIDI input the controller was heard on.
    pub endpoint_name: String,
    pub inputs: Vec<ControllerInput>,
}

/// An input's name as the package matches it: without the client and port
/// numbers ALSA appends ("24:0"), which change between boots, and folded.
pub fn endpoint_match_name(endpoint_name: &str) -> String {
    let trimmed = endpoint_name.trim();
    let without_port = match trimmed.rsplit_once(' ') {
        Some((head, tail))
            if tail.split_once(':').is_some_and(|(client, port)| {
                !client.is_empty()
                    && !port.is_empty()
                    && client.bytes().all(|byte| byte.is_ascii_digit())
                    && port.bytes().all(|byte| byte.is_ascii_digit())
            }) =>
        {
            head.trim_end()
        }
        _ => trimmed,
    };
    without_port.to_ascii_lowercase()
}

/// A package id from a controller's name: `user.oxygen-49`.
pub fn user_controller_id(name: &str) -> String {
    let mut slug = String::new();
    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
        if slug.len() >= 48 {
            break;
        }
    }
    let slug = slug.trim_end_matches('-');
    format!(
        "{USER_CONTROLLER_PREFIX}{}",
        if slug.is_empty() { "controller" } else { slug }
    )
}

/// The package for a player's controller, at a given version.
pub fn user_controller_manifest(
    request: &UserControllerRequest,
    controller_id: &str,
    version: &Version,
) -> Result<ControllerPackageManifest, PackageError> {
    let match_name = endpoint_match_name(&request.endpoint_name);
    if match_name.is_empty() {
        return Err(PackageError::InvalidManifest(
            "a controller needs the MIDI input it was heard on".into(),
        ));
    }
    let manifest = ControllerPackageManifest {
        schema_version: CONTROLLER_PACKAGE_SCHEMA_VERSION_2,
        kind: "controller".into(),
        id: controller_id.into(),
        name: request.name.trim().into(),
        vendor: request
            .vendor
            .as_deref()
            .map(str::trim)
            .filter(|vendor| !vendor.is_empty())
            .map(Into::into),
        version: version.to_string(),
        controller_api: USER_CONTROLLER_API.into(),
        runtime: declarative_runtime(),
        permissions: input_only_permissions(),
        devices: vec![DeviceMatcher {
            id: "device".into(),
            usb_vendor_id: 0,
            usb_product_ids: Vec::new(),
            product_names: Vec::new(),
            endpoints: vec![EndpointMatcher {
                role: EndpointRole::PerformanceInput,
                name_contains: vec![match_name.clone()],
                name_contains_any: Vec::new(),
                name_ends_with: None,
                // A word the chosen port carries itself is no sign of
                // another port.
                exclude_contains: AUXILIARY_PORT_WORDS
                    .iter()
                    .filter(|word| !match_name.contains(*word))
                    .map(|word| (*word).into())
                    .collect(),
            }],
            sysex_identity: None,
        }],
        inputs: request.inputs.clone(),
        roles: Vec::new(),
        actions: Vec::new(),
        source_id: None,
        on_connect: Vec::new(),
        surfaces: Vec::new(),
        host_controls: Vec::new(),
        host_actions: Vec::new(),
        semantic_profile: None,
        integrity: None,
        settings: Vec::new(),
    };
    manifest.validate()?;
    Ok(manifest)
}

impl PackageStore {
    /// Makes or saves again a player's controller package, and installs it
    /// enabled. Saving again gives the next version; a new package whose id
    /// is taken gets a number after it.
    pub fn save_user_controller(
        &self,
        request: &UserControllerRequest,
    ) -> Result<InstalledController, PackageError> {
        let (controller_id, version) = match &request.controller_id {
            Some(id) => {
                if !id.starts_with(USER_CONTROLLER_PREFIX) {
                    return Err(PackageError::InvalidManifest(format!(
                        "{id} is not a controller a player made"
                    )));
                }
                let installed = self.resolve(id)?;
                if installed.record.trust != PackageTrust::Local {
                    return Err(PackageError::InvalidManifest(format!(
                        "{id} was not made here and cannot be saved over"
                    )));
                }
                let mut version = Version::parse(&installed.record.version).map_err(|error| {
                    PackageError::InvalidManifest(format!("invalid installed version: {error}"))
                })?;
                version.patch += 1;
                version.pre = semver::Prerelease::EMPTY;
                version.build = semver::BuildMetadata::EMPTY;
                (id.clone(), version)
            }
            None => {
                let base = user_controller_id(&request.name);
                let mut id = base.clone();
                let mut number = 2;
                while self.resolve(&id).is_ok() {
                    id = format!("{base}-{number}");
                    number += 1;
                }
                (id, Version::new(1, 0, 0))
            }
        };
        let manifest = user_controller_manifest(request, &controller_id, &version)?;
        let staging = self.root().join(format!(
            ".user-controller-{}-{}",
            std::process::id(),
            controller_id
        ));
        if staging.exists() {
            fs::remove_dir_all(&staging)?;
        }
        fs::create_dir_all(&staging)?;
        let written = toml::to_string_pretty(&manifest)
            .map_err(PackageError::TomlSerialize)
            .and_then(|text| {
                fs::write(staging.join(CONTROLLER_MANIFEST_FILE), text).map_err(PackageError::from)
            });
        let installed =
            written.and_then(|()| self.install_directory(&staging, PackageTrust::Local));
        let _ = fs::remove_dir_all(&staging);
        installed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{InputKind, InputMidi};

    struct Root(std::path::PathBuf);

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn root(name: &str) -> Root {
        let path = std::env::temp_dir().join(format!(
            "rackforge-user-controller-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        Root(path)
    }

    fn request(name: &str) -> UserControllerRequest {
        UserControllerRequest {
            controller_id: None,
            name: name.into(),
            vendor: Some("M-Audio".into()),
            endpoint_name: "Oxygen 49:Oxygen 49 MIDI 1 24:0".into(),
            inputs: vec![ControllerInput {
                id: "cc-1-74".into(),
                name: "Knob 1".into(),
                kind: InputKind::Knob,
                group: Some("Knobs".into()),
                midi: InputMidi {
                    channel: 0,
                    cc: Some(74),
                    ..InputMidi::default()
                },
                button: None,
                encoder: None,
            }],
        }
    }

    #[test]
    fn a_port_is_matched_by_its_name_not_its_numbers() {
        assert_eq!(
            endpoint_match_name("Oxygen 49:Oxygen 49 MIDI 1 24:0"),
            "oxygen 49:oxygen 49 midi 1"
        );
        assert_eq!(endpoint_match_name("  Oxygen 49 "), "oxygen 49");
        assert_eq!(endpoint_match_name("Launchkey 1:2"), "launchkey");
        assert_eq!(endpoint_match_name("Mixer 2 a:b"), "mixer 2 a:b");
    }

    #[test]
    fn a_name_becomes_a_user_id() {
        assert_eq!(
            user_controller_id("Oxygen 49 (MK V)"),
            "user.oxygen-49-mk-v"
        );
        assert_eq!(user_controller_id("!!!"), "user.controller");
    }

    #[test]
    fn a_player_s_controller_is_installed_and_saved_again() {
        let root = root("save");
        let store = PackageStore::new(&root.0);
        let installed = store.save_user_controller(&request("Oxygen 49")).unwrap();
        assert_eq!(installed.record.id, "user.oxygen-49");
        assert_eq!(installed.record.version, "1.0.0");
        assert!(installed.record.enabled);
        let manifest = installed.package.manifest();
        assert!(manifest.is_declarative());
        assert_eq!(manifest.inputs.len(), 1);
        assert!(!manifest.permissions.midi_output);

        // The port it was heard on is the one it answers, not the DAW port.
        let binding = store
            .resolve_declarative_input("Oxygen 49:Oxygen 49 MIDI 1 28:0")
            .unwrap()
            .unwrap();
        assert_eq!(binding.controller_id, "user.oxygen-49");
        assert!(
            store
                .resolve_declarative_input("Oxygen 49:Oxygen 49 DAW 28:1")
                .unwrap()
                .is_none()
        );

        let mut again = request("Oxygen 49");
        again.controller_id = Some("user.oxygen-49".into());
        again.inputs[0].name = "Cutoff".into();
        let saved = store.save_user_controller(&again).unwrap();
        assert_eq!(saved.record.version, "1.0.1");
        assert_eq!(saved.package.manifest().inputs[0].name, "Cutoff");

        // Another controller of the same name gets a number, not the first's.
        let second = store.save_user_controller(&request("Oxygen 49")).unwrap();
        assert_eq!(second.record.id, "user.oxygen-49-2");
    }

    #[test]
    fn a_published_package_is_never_saved_over() {
        let root = root("published");
        let store = PackageStore::new(&root.0);
        let mut request = request("Oxygen 49");
        request.controller_id = Some("org.rackforge.arturia-keylab-essential-mk3".into());
        assert!(store.save_user_controller(&request).is_err());
    }

    #[test]
    fn a_controller_without_an_input_to_match_is_refused() {
        let mut empty = request("Oxygen 49");
        empty.endpoint_name = "   ".into();
        assert!(
            user_controller_manifest(&empty, "user.oxygen-49", &Version::new(1, 0, 0)).is_err()
        );
    }
}
