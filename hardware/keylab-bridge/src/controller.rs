use rackforge_controller_api::{
    ControllerDriver, ControllerProfile, GestureCapabilities, HostActionBinding, HostActionTarget,
    HostControlBinding, HostControlTarget, LITTLE_V1, MidiButtonBinding, MidiControlChangeBinding,
    SurfaceImplementation, SurfaceQuality, negotiate_surface,
};
use std::sync::OnceLock;

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
            host_controls: vec![
                HostControlBinding {
                    target: HostControlTarget::MasterLevel,
                    midi_cc: MidiControlChangeBinding {
                        channel: 0,
                        controller: 113,
                    },
                },
                HostControlBinding {
                    target: HostControlTarget::MasterPan,
                    midi_cc: MidiControlChangeBinding {
                        channel: 0,
                        controller: 104,
                    },
                },
            ],
            host_actions: vec![HostActionBinding {
                target: HostActionTarget::KeyboardParts,
                midi_cc: MidiButtonBinding {
                    channel: 0,
                    controller: 119,
                    press_value: 127,
                    release_value: 0,
                },
            }],
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

fn is_main_midi_endpoint(name: &str) -> bool {
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
        assert_eq!(
            driver.profile().host_controls,
            vec![
                HostControlBinding {
                    target: HostControlTarget::MasterLevel,
                    midi_cc: MidiControlChangeBinding {
                        channel: 0,
                        controller: 113,
                    },
                },
                HostControlBinding {
                    target: HostControlTarget::MasterPan,
                    midi_cc: MidiControlChangeBinding {
                        channel: 0,
                        controller: 104,
                    },
                },
            ]
        );
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
}
