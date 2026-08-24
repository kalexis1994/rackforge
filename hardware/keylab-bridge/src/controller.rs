use rackforge_control_profile::{CONTROL_PROFILE_SCHEMA_VERSION, SemanticControlId, roles};
use rackforge_controller_api::{
    ControllerDriver, ControllerProfile, GestureCapabilities, HostActionBinding, HostActionTarget,
    LITTLE_V1, MidiButtonBinding, MidiControlChangeBinding, SemanticControlBinding,
    SemanticControlMode, SemanticControlProfile, SurfaceImplementation, SurfaceQuality,
    negotiate_surface,
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
        PROFILE.get_or_init(|| ControllerProfile {
            id: "arturia.keylab-essential-mk3".into(),
            name: "Arturia KeyLab Essential mk3".into(),
            driver_id: "org.rackforge.arturia-keylab-essential-mk3".into(),
            surfaces: vec![SurfaceImplementation {
                layout_id: LITTLE_V1.into(),
                quality: SurfaceQuality::Native,
                priority: 0,
                gestures: GestureCapabilities {
                    soft_key_long_press: true,
                    emergency_home_chord: true,
                },
            }],
            host_controls: Vec::new(),
            host_actions: vec![HostActionBinding {
                target: HostActionTarget::KeyboardParts,
                midi_cc: MidiButtonBinding {
                    channel: 0,
                    controller: 119,
                    press_value: 127,
                    release_value: 0,
                },
            }],
            semantic_profile: Some(SemanticControlProfile {
                schema_version: CONTROL_PROFILE_SCHEMA_VERSION,
                source_id: "controller.arturia.keylab-essential-mk3.midi".into(),
                controls: [
                    (roles::SYNTH_OSCILLATOR_PULSE_WIDTH, 96),
                    (roles::SYNTH_OSCILLATOR_SUB_LEVEL, 97),
                    (roles::SYNTH_OSCILLATOR_NOISE_LEVEL, 98),
                    (roles::SYNTH_FILTER_ENVELOPE_AMOUNT, 99),
                    (roles::SYNTH_FILTER_LFO_AMOUNT, 100),
                    (roles::SYNTH_FILTER_KEY_TRACKING, 101),
                    (roles::SYNTH_LFO_DELAY, 102),
                    (roles::SYNTH_AMPLIFIER_LEVEL, 103),
                    (roles::SYNTH_AMP_ENVELOPE_ATTACK, 105),
                    (roles::SYNTH_AMP_ENVELOPE_DECAY, 106),
                    (roles::SYNTH_AMP_ENVELOPE_SUSTAIN, 107),
                    (roles::SYNTH_AMP_ENVELOPE_RELEASE, 108),
                    (roles::SYNTH_FILTER_CUTOFF, 109),
                    (roles::SYNTH_FILTER_RESONANCE, 110),
                    (roles::SYNTH_LFO_RATE, 111),
                    (roles::SYNTH_LFO_DEPTH, 112),
                    (roles::RACKFORGE_MASTER_LEVEL, 113),
                ]
                .into_iter()
                .map(|(role, controller)| SemanticControlBinding {
                    role: SemanticControlId::new(role).expect("built-in semantic role is valid"),
                    midi_cc: MidiControlChangeBinding {
                        channel: 0,
                        controller,
                    },
                    invert: false,
                    mode: SemanticControlMode::Absolute,
                })
                .chain(std::iter::once(SemanticControlBinding {
                    role: SemanticControlId::new(roles::RACKFORGE_MASTER_PAN)
                        .expect("built-in semantic role is valid"),
                    midi_cc: MidiControlChangeBinding {
                        channel: 0,
                        controller: 104,
                    },
                    invert: false,
                    mode: SemanticControlMode::Relative,
                }))
                .collect(),
            }),
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

pub fn device_matchers() -> &'static [DeviceMatcher] {
    static DEVICES: OnceLock<Vec<DeviceMatcher>> = OnceLock::new();
    DEVICES.get_or_init(|| {
        toml::from_str::<ControllerPackageManifest>(PACKAGE_MANIFEST)
            .expect("the embedded KeyLab .rfcontroller manifest must remain valid")
            .devices
    })
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

pub fn is_main_midi_endpoint(name: &str) -> bool {
    let trimmed = name.trim();
    let endpoint = trimmed
        .rsplit_once(' ')
        .filter(|(_, suffix)| is_alsa_address(suffix))
        .map_or(trimmed, |(prefix, _)| prefix);
    let folded = endpoint.to_ascii_lowercase();
    (folded.contains("kl essential") || folded.contains("keylab"))
        && folded.trim_end().ends_with("midi")
        && !folded.contains("mcu")
        && !folded.contains("hui")
        && !folded.contains("dinthru")
        && !folded.contains(" alv")
}

pub fn is_keylab_endpoint(name: &str) -> bool {
    let folded = name.trim().to_ascii_lowercase();
    folded.contains("kl essential") || folded.contains("keylab")
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
    use std::collections::BTreeMap;

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
            vec![HostActionBinding {
                target: HostActionTarget::KeyboardParts,
                midi_cc: MidiButtonBinding {
                    channel: 0,
                    controller: 119,
                    press_value: 127,
                    release_value: 0,
                },
            }]
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
    }
}
