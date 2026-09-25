use rackforge_controller_api::{
    ControllerDriver, ControllerProfile, GestureCapabilities, LITTLE_V1, SurfaceImplementation,
    SurfaceQuality, SurfaceViewport, negotiate_surface,
};
use rackforge_controller_package::{ControllerPackageManifest, DeviceMatcher};
use std::sync::OnceLock;

pub const PACKAGE_MANIFEST: &str = include_str!(
    "../../controllers/arturia-keylab-essential-mk3/package/rackforge-controller.toml"
);

pub struct KeyLabEssentialMk3;

impl ControllerDriver for KeyLabEssentialMk3 {
    fn profile(&self) -> &ControllerProfile {
        static PROFILE: OnceLock<ControllerProfile> = OnceLock::new();
        PROFILE.get_or_init(|| {
            // The part key and the encoder and fader roles are the package's:
            // the driver reports what the manifest lowers to rather than a
            // second copy of it, so the two cannot drift apart.
            let package = package_manifest().profile();
            ControllerProfile {
                id: "arturia.keylab-essential-mk3".into(),
                name: "Arturia KeyLab Essential mk3".into(),
                driver_id: "org.rackforge.arturia-keylab-essential-mk3".into(),
                surfaces: vec![SurfaceImplementation {
                    layout_id: LITTLE_V1.into(),
                    quality: SurfaceQuality::Native,
                    priority: 0,
                    viewport: SurfaceViewport::little_reference(),
                    gestures: GestureCapabilities {
                        soft_key_long_press: true,
                        emergency_home_chord: true,
                    },
                }],
                host_controls: package.host_controls,
                host_actions: package.host_actions,
                semantic_profile: package.semantic_profile,
            }
        })
    }

    fn matches_display_output(&self, port_name: &str) -> bool {
        is_main_midi_endpoint(port_name)
    }

    fn matches_surface_input(&self, port_name: &str) -> bool {
        is_main_midi_endpoint(port_name)
    }
}

static KEYLAB_ESSENTIAL_MK3: KeyLabEssentialMk3 = KeyLabEssentialMk3;
static DRIVERS: [&'static dyn ControllerDriver; 1] = [&KEYLAB_ESSENTIAL_MK3];

pub fn package_profile() -> &'static ControllerProfile {
    KEYLAB_ESSENTIAL_MK3.profile()
}

/// The embedded `.rfcontroller` manifest, parsed and validated once.
fn package_manifest() -> &'static ControllerPackageManifest {
    static MANIFEST: OnceLock<ControllerPackageManifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let manifest = toml::from_str::<ControllerPackageManifest>(PACKAGE_MANIFEST)
            .expect("the embedded KeyLab .rfcontroller manifest must parse");
        manifest
            .validate()
            .expect("the embedded KeyLab .rfcontroller manifest must remain valid");
        manifest
    })
}

/// Whether a message comes from one of the package's own controls -- a
/// pad's note, a wheel -- rather than a key: what a player's map may bind,
/// and so what may have moved a parameter. A key's note is not one of them.
pub fn is_package_control_message(message: &[u8]) -> bool {
    use rackforge_controller_package::InputMessage;
    let (Some(&status), Some(&data1)) = (message.first(), message.get(1)) else {
        return false;
    };
    let channel = status & 0x0f;
    let heard = match status & 0xf0 {
        0x80 | 0x90 => InputMessage::Note {
            channel,
            note: data1,
        },
        0xb0 => InputMessage::ControlChange {
            channel,
            controller: data1,
        },
        0xe0 => InputMessage::PitchBend { channel },
        _ => return false,
    };
    package_manifest()
        .inputs
        .iter()
        .any(|input| input.midi.message() == Ok(heard))
}

pub fn device_matchers() -> &'static [DeviceMatcher] {
    &package_manifest().devices
}

pub fn matches_usb_device(vendor_id: u16, product_id: u16) -> bool {
    device_matchers().iter().any(|device| {
        device.usb_vendor_id == vendor_id && device.usb_product_ids.contains(&product_id)
    })
}

pub fn matches_product_name(name: &str) -> bool {
    let folded = name.trim().to_ascii_lowercase();
    !folded.is_empty()
        && device_matchers().iter().any(|device| {
            device.product_names.iter().any(|product| {
                let product = product.trim().to_ascii_lowercase();
                folded == product || folded.contains(&product)
            })
        })
}

pub fn matches_endpoint_name_hint(name: &str) -> bool {
    let folded = name.trim().to_ascii_lowercase();
    !folded.is_empty()
        && device_matchers().iter().any(|device| {
            device.endpoints.iter().any(|endpoint| {
                endpoint
                    .name_contains
                    .iter()
                    .all(|part| folded.contains(&part.to_ascii_lowercase()))
                    && (endpoint.name_contains_any.is_empty()
                        || endpoint
                            .name_contains_any
                            .iter()
                            .any(|part| folded.contains(&part.to_ascii_lowercase())))
                    && endpoint
                        .exclude_contains
                        .iter()
                        .all(|part| !folded.contains(&part.to_ascii_lowercase()))
            })
        })
}

pub fn display_driver(port_name: &str) -> Option<&'static dyn ControllerDriver> {
    DRIVERS
        .iter()
        .copied()
        .find(|driver| driver.matches_display_output(port_name))
}

pub fn surface_input_driver(port_name: &str) -> Option<&'static dyn ControllerDriver> {
    DRIVERS
        .iter()
        .copied()
        .find(|driver| driver.matches_surface_input(port_name))
}

pub fn little_driver(port_name: &str) -> Option<&'static dyn ControllerDriver> {
    let driver = display_driver(port_name)?;
    let layouts = [LITTLE_V1.to_owned()];
    negotiate_surface(driver.profile(), &layouts)?;
    Some(driver)
}

/// The one endpoint that carries notes and the display, told apart from the
/// device's other ports.
///
/// Two naming conventions have to be read, and they mark the extra ports in
/// opposite places. ALSA appends the endpoint and its address, so the same
/// keyboard arrives as `KL Essential 61 mk3 MIDI 28:0` beside
/// `KL Essential 61 mk3 MCU/HUI 28:2`, and the main one is the one ending in
/// `MIDI`. Windows reports the device's product string bare --
/// `KL Essential 61 mk3`, read from a running host -- and numbers any extra
/// ports ahead of it as `MIDIIN2 (...)`.
///
/// Requiring the `MIDI` suffix everywhere therefore accepted the right port
/// on the appliance and rejected every port on Windows, where the driver sat
/// printing "Esperando el KeyLab Essential mk3..." forever and neither the
/// display nor the key colours were ever sent. Every test this function had
/// used an ALSA name, so nothing caught it.
pub fn is_main_midi_endpoint(name: &str) -> bool {
    let trimmed = name.trim();
    let address = trimmed
        .rsplit_once(' ')
        .filter(|(_, suffix)| is_alsa_address(suffix));
    let endpoint = address.map_or(trimmed, |(prefix, _)| prefix);
    let folded = endpoint.to_ascii_lowercase();
    if !names_keylab_essential_mk3(&folded) {
        return false;
    }
    if folded.contains("mcu")
        || folded.contains("hui")
        || folded.contains("dinthru")
        || folded.contains(" alv")
    {
        return false;
    }
    if address.is_some() {
        // An ALSA name: the suffix names the endpoint, so it has to say MIDI.
        return folded.trim_end().ends_with("midi");
    }
    // Otherwise the product name is the port and any extras are numbered
    // ahead of it. A bare `MIDIIN`/`MIDIOUT` with no number is the only port
    // there is, so only a numbered one is an extra.
    !is_numbered_secondary_port(&folded)
}

/// `MIDIIN2 (...)` and `MIDIOUT3 (...)`: how Windows names a multi-port
/// device's second and later endpoints.
fn is_numbered_secondary_port(folded: &str) -> bool {
    for prefix in ["midiin", "midiout"] {
        if let Some(rest) = folded.strip_prefix(prefix) {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(index) = digits.parse::<u32>() {
                return index > 1;
            }
        }
    }
    false
}

pub fn is_keylab_endpoint(name: &str) -> bool {
    names_keylab_essential_mk3(&name.trim().to_ascii_lowercase())
}

/// The KeyLab Essential mk3, in either spelling its ports carry -- "KL
/// Essential 61 mk3", or the product's "KeyLab Essential 61 mk3" -- and no
/// other Arturia keyboard. "keylab" alone also named a KeyLab mkII's or
/// mk3's main port and the first KeyLab Essential's: the driver would have
/// sent them this one's DAW program, display and LEDs, and with two of them
/// plugged in it found the choice ambiguous and drove neither.
fn names_keylab_essential_mk3(folded: &str) -> bool {
    (folded.contains("kl essential") || folded.contains("keylab essential")) && folded.contains("mk3")
}

fn is_alsa_address(value: &str) -> bool {
    value.split_once(':').is_some_and(|(client, port)| {
        !client.is_empty()
            && !port.is_empty()
            && client.bytes().all(|byte| byte.is_ascii_digit())
            && port.bytes().all(|byte| byte.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pad_is_a_control_and_a_key_is_not() {
        // Pad 1, bank 1: note 40 on channel 11, struck and let go.
        assert!(is_package_control_message(&[0x9a, 40, 90]));
        assert!(is_package_control_message(&[0x8a, 40, 0]));
        // Pad 1, bank 2, and the mod wheel.
        assert!(is_package_control_message(&[0x9a, 48, 30]));
        assert!(is_package_control_message(&[0xb0, 1, 64]));
        // Middle C on the keys, and note 40 on the keys' channel.
        assert!(!is_package_control_message(&[0x90, 60, 100]));
        assert!(!is_package_control_message(&[0x90, 40, 100]));
    }
    use rackforge_control_profile::roles;
    use rackforge_controller_api::{
        HostActionBinding, HostActionTarget, MidiButtonBinding, SemanticControlMode,
    };
    use std::collections::BTreeMap;

    #[test]
    fn the_main_endpoint_is_found_under_both_naming_conventions() {
        // ALSA, as the appliance sees it. Unchanged behaviour.
        assert!(is_main_midi_endpoint("KL Essential 61 mk3 MIDI 28:0"));
        assert!(!is_main_midi_endpoint("KL Essential 61 mk3 MCU/HUI 28:2"));
        assert!(!is_main_midi_endpoint("KL Essential 61 mk3 DINTHRU 28:1"));
        assert!(!is_main_midi_endpoint("KL Essential 61 mk3 ALV 28:3"));

        // Windows, copied from a running host's log rather than guessed:
        // `DESKTOP_MIDI_SOURCE_CONNECTED name="KL Essential 61 mk3"`. The
        // product string arrives bare, with no endpoint word to end in, and
        // requiring one rejected it.
        assert!(is_main_midi_endpoint("KL Essential 61 mk3"));
        assert!(!is_main_midi_endpoint("MIDIIN2 (KL Essential 61 mk3)"));
        assert!(!is_main_midi_endpoint("MIDIOUT2 (KL Essential 61 mk3)"));

        // A device that is not a KeyLab stays out under either convention.
        assert!(!is_main_midi_endpoint("Unknown USB MIDI 31:0"));
        assert!(!is_main_midi_endpoint("MIDIIN2 (Some Other Keyboard)"));
    }

    #[test]
    fn other_keylabs_are_never_driven_as_this_one() {
        // The product's own spelling, as the package names the device.
        assert!(is_main_midi_endpoint("KeyLab Essential 61 mk3 MIDI 28:0"));
        assert!(is_main_midi_endpoint("KL Essential 49 mk3:KL Essential 49 mk3 MIDI 24:0"));
        // Other KeyLabs' main ports end in MIDI too, and speak other
        // protocols: a KeyLab mkII or mk3, and the first KeyLab Essential.
        for name in [
            "KeyLab mkII 61:KeyLab mkII 61 MIDI 24:0",
            "KeyLab 61 mk3:KeyLab 61 mk3 MIDI 24:0",
            "KeyLab 61 mk3",
            "Arturia KeyLab Essential 61:Arturia KeyLab Essential 61 MIDI 24:0",
            "Arturia KeyLab Essential 61",
            "KeyLab 88:KeyLab 88 MIDI 24:0",
        ] {
            assert!(!is_main_midi_endpoint(name), "{name}");
            assert!(!is_keylab_endpoint(name), "{name}");
            assert!(display_driver(name).is_none(), "{name}");
        }
        assert!(is_keylab_endpoint("MIDIIN2 (KL Essential 61 mk3)"));
    }

    #[test]
    fn the_windows_port_negotiates_little() {
        // The desktop gates LITTLE on `little_driver`, and the driver process
        // waits on `display_driver`, so the fix has to reach through the
        // lookup rather than stopping at the name test.
        let driver = little_driver("KL Essential 61 mk3").unwrap();
        assert_eq!(driver.profile().surfaces[0].layout_id, LITTLE_V1);
        assert!(display_driver("KL Essential 61 mk3").is_some());
        assert!(little_driver("MIDIIN2 (KL Essential 61 mk3)").is_none());
    }

    #[test]
    fn unknown_midi_devices_never_receive_a_display_driver() {
        assert!(display_driver("Unknown USB MIDI 31:0").is_none());
        assert!(little_driver("Unknown USB MIDI 31:0").is_none());
    }

    #[test]
    fn keylab_main_endpoint_is_certified_for_little() {
        let driver = little_driver("KL Essential 61 mk3 MIDI 28:0").unwrap();
        assert_eq!(driver.profile().surfaces[0].layout_id, LITTLE_V1);
        assert_eq!(driver.profile().surfaces[0].quality, SurfaceQuality::Native);
        assert!(driver.profile().surfaces[0].gestures.soft_key_long_press);
        assert!(driver.profile().surfaces[0].gestures.emergency_home_chord);
        assert!(driver.profile().host_controls.is_empty());
        assert_eq!(
            driver.profile().host_actions,
            vec![HostActionBinding::control_change(
                HostActionTarget::KeyboardParts,
                MidiButtonBinding {
                    channel: 0,
                    controller: 119,
                    press_value: 127,
                    release_value: 0,
                },
            )]
        );
    }

    #[test]
    fn auxiliary_keylab_ports_are_not_surface_endpoints() {
        assert!(display_driver("KL Essential 61 mk3 DINTHRU 28:1").is_none());
        assert!(display_driver("KL Essential 61 mk3 MCU/HUI 28:2").is_none());
        assert!(display_driver("KL Essential 61 mk3 ALV 28:3").is_none());
    }

    #[test]
    fn rackforge_profile_maps_all_nine_encoders_and_faders() {
        let semantic = package_profile().semantic_profile.as_ref().unwrap();
        let controls = semantic
            .controls
            .iter()
            .map(|binding| (binding.midi_cc.controller, binding.role.as_str()))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(controls.len(), 18);
        assert_eq!(
            controls.get(&96),
            Some(&roles::SYNTH_OSCILLATOR_PULSE_WIDTH)
        );
        assert_eq!(controls.get(&103), Some(&roles::SYNTH_AMPLIFIER_LEVEL));
        assert_eq!(controls.get(&105), Some(&roles::SYNTH_AMP_ENVELOPE_ATTACK));
        assert_eq!(controls.get(&112), Some(&roles::SYNTH_LFO_DEPTH));
        assert_eq!(controls.get(&104), Some(&roles::RACKFORGE_MASTER_PAN));
        assert_eq!(controls.get(&113), Some(&roles::RACKFORGE_MASTER_LEVEL));
        assert_eq!(
            semantic
                .controls
                .iter()
                .find(|binding| binding.midi_cc.controller == 104)
                .unwrap()
                .mode,
            SemanticControlMode::Relative
        );
        // Everything else is read absolutely, as it was under schema 1, and
        // the source keeps the identity learnt links refer to.
        assert!(
            semantic
                .controls
                .iter()
                .filter(|binding| binding.midi_cc.controller != 104)
                .all(|binding| binding.mode == SemanticControlMode::Absolute)
        );
        assert_eq!(
            semantic.source_id,
            "controller.arturia.keylab-essential-mk3.midi"
        );
    }

    #[test]
    fn the_package_names_every_control_it_maps() {
        let manifest = package_manifest();
        assert_eq!(manifest.schema_version, 2);
        // Nine knobs, nine faders, sixteen pads, nine buttons, two wheels:
        // what the hardware sends that the driver does not own.
        assert_eq!(manifest.inputs.len(), 45);
        assert!(manifest.inputs.iter().any(|input| input.id == "part"));
        let pads = manifest
            .inputs
            .iter()
            .filter(|input| input.id.starts_with("pad-"))
            .collect::<Vec<_>>();
        assert_eq!(pads.len(), 16);
        // MIDI channel 11, zero-based.
        assert!(pads.iter().all(|pad| pad.midi.channel == 10));
        // The OLED buttons, the main encoder and the transport the driver
        // keeps are never offered for a map.
        for owned in [20, 21, 22, 23, 24, 44, 45, 46, 47, 116, 117] {
            assert!(
                manifest
                    .inputs
                    .iter()
                    .all(|input| input.midi.cc != Some(owned)),
                "CC {owned} belongs to the driver"
            );
        }
    }

    #[test]
    fn package_identity_matches_the_supported_android_usb_device() {
        assert!(matches_usb_device(0x1c75, 0x028c));
        assert!(!matches_usb_device(0x1c75, 0xffff));
        assert!(matches_product_name("KeyLab Essential 61 mk3"));
        assert!(matches_product_name("KeyLab Essential 61 mk3 MIDI"));
        assert!(!matches_product_name("Generic USB MIDI"));
        assert!(matches_endpoint_name_hint("KL Essential 61 mk3"));
        assert!(!matches_endpoint_name_hint("KL Essential 61 mk3 MCU/HUI"));
        assert!(!matches_endpoint_name_hint("KeyLab mkII 61 MIDI"));
        assert!(!matches_endpoint_name_hint("KeyLab 61 mk3 MIDI"));
    }
}
