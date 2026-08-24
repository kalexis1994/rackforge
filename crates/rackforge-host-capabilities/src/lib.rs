//! What each RackForge host can do, declared once and checked.
//!
//! RackForge runs the same instruments and the same performances on Windows,
//! Android, a Raspberry Pi and, now, a web page. Keeping that promise is
//! harder than making it: a feature added to one host is easy to forget on the
//! others, and the gap is usually discovered by a performer rather than by a
//! developer.
//!
//! This crate is the record that stops that happening quietly. Every host
//! declares, for every capability, one of three answers:
//!
//! * [`Support::Yes`] — the host implements it;
//! * [`Support::Planned`] — it does not yet, with the reason it is missing;
//! * [`Support::Unavailable`] — it cannot, with the reason it never will.
//!
//! There is no fourth answer and no default, so a new capability cannot be
//! added for one host without every other host stating where it stands. The
//! tests enforce three things: that no cell is left out, that the capabilities
//! marked [`CORE`] are present everywhere, and that
//! `docs/PLATFORM_PARITY.md` matches this declaration exactly — the document
//! is generated from here, so it cannot drift.
//!
//! The declaration is a claim, not a proof. For the browser host the claim is
//! also probed: `tools/check-browser-host.mjs` exercises every capability it
//! reports as supported, so a lie there fails CI rather than a performance.

use std::fmt;

/// One thing a performer can do with RackForge.
///
/// Capabilities describe the product, not the implementation: "play an
/// instrument", not "load a wasm component". They are stable identifiers —
/// renaming one is a change to the record, not a refactor.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Capability {
    /// Choose an instrument and play it from PLAY.
    PlayInstrument,
    /// Choose the program an instrument is playing.
    SelectProgram,
    /// Set master level and pan while playing.
    MasterLevelAndPan,
    /// Save, load, rename, delete, import and export host presets.
    HostPresets,
    /// Read and write a plugin's parameters.
    PluginParameters,
    /// Create and edit Racks, Songs and Setlists.
    PerformanceLibrary,
    /// Play a Rack: every slot rendered and mixed, not just one instrument.
    RackRendering,
    /// Edit the state of one Rack slot without disturbing what PLAY is doing.
    RackSlotStateEditing,
    /// Create, preview and save Custom Programs through a plugin's editor.
    ProgramDrafts,
    /// Audition a program without losing the one that was selected.
    Auditions,
    /// Install a portable `.rfplugin` package.
    PluginInstall,
    /// Remove an installed plugin, with its presets and private data.
    PluginRemoval,
    /// Give a plugin a resource it declares, such as a sound library.
    PluginResources,
    /// Show a plugin's own PLAY and CONFIG web interfaces.
    PluginWebSurfaces,
    /// Drive hardware displays, encoders, buttons and pads from a
    /// `.rfcontroller` package.
    ControllerPackages,
    /// Play from a connected MIDI controller.
    MidiInput,
    /// Send MIDI to hardware, which controller surfaces need.
    MidiOutput,
    /// Notice a controller being connected or disconnected while running.
    MidiHotplug,
    /// Play from an on-screen keyboard or pads.
    VirtualMidi,
    /// Choose the audio device and its buffer settings.
    AudioDeviceSelection,
    /// Keep instruments, performances and settings between runs.
    PersistentStorage,
    /// Restore the previous mode, instrument, program and master settings on
    /// the next start.
    SessionRestore,
    /// Work with no network connection.
    OfflineOperation,
    /// Stop a plugin that stops responding, instead of losing the audio
    /// thread with it.
    RuntimeMetering,
}

impl Capability {
    /// Every capability, in the order the parity document lists them.
    pub const ALL: &'static [Self] = &[
        Self::PlayInstrument,
        Self::SelectProgram,
        Self::MasterLevelAndPan,
        Self::VirtualMidi,
        Self::MidiInput,
        Self::MidiOutput,
        Self::MidiHotplug,
        Self::PluginParameters,
        Self::HostPresets,
        Self::ProgramDrafts,
        Self::Auditions,
        Self::PerformanceLibrary,
        Self::RackRendering,
        Self::RackSlotStateEditing,
        Self::PluginInstall,
        Self::PluginRemoval,
        Self::PluginResources,
        Self::PluginWebSurfaces,
        Self::ControllerPackages,
        Self::AudioDeviceSelection,
        Self::PersistentStorage,
        Self::SessionRestore,
        Self::OfflineOperation,
        Self::RuntimeMetering,
    ];

    /// Stable identifier, used by the parity document and by the browser
    /// host's capability report.
    pub const fn id(self) -> &'static str {
        match self {
            Self::PlayInstrument => "play_instrument",
            Self::SelectProgram => "select_program",
            Self::MasterLevelAndPan => "master_level_and_pan",
            Self::HostPresets => "host_presets",
            Self::PluginParameters => "plugin_parameters",
            Self::PerformanceLibrary => "performance_library",
            Self::RackRendering => "rack_rendering",
            Self::RackSlotStateEditing => "rack_slot_state_editing",
            Self::ProgramDrafts => "program_drafts",
            Self::Auditions => "auditions",
            Self::PluginInstall => "plugin_install",
            Self::PluginRemoval => "plugin_removal",
            Self::PluginResources => "plugin_resources",
            Self::PluginWebSurfaces => "plugin_web_surfaces",
            Self::ControllerPackages => "controller_packages",
            Self::MidiInput => "midi_input",
            Self::MidiOutput => "midi_output",
            Self::MidiHotplug => "midi_hotplug",
            Self::VirtualMidi => "virtual_midi",
            Self::AudioDeviceSelection => "audio_device_selection",
            Self::PersistentStorage => "persistent_storage",
            Self::SessionRestore => "session_restore",
            Self::OfflineOperation => "offline_operation",
            Self::RuntimeMetering => "runtime_metering",
        }
    }

    /// One line a performer would recognise, used as the document's row label.
    pub const fn summary(self) -> &'static str {
        match self {
            Self::PlayInstrument => "Choose an instrument and play it",
            Self::SelectProgram => "Choose the program an instrument plays",
            Self::MasterLevelAndPan => "Set master level and pan",
            Self::HostPresets => "Save, load, rename, delete, import and export host presets",
            Self::PluginParameters => "Read and write plugin parameters",
            Self::PerformanceLibrary => "Create and edit Racks, Songs and Setlists",
            Self::RackRendering => "Play a Rack, with every slot rendered",
            Self::RackSlotStateEditing => "Edit one Rack slot without disturbing PLAY",
            Self::ProgramDrafts => "Create, preview and save Custom Programs",
            Self::Auditions => "Audition a program and keep the selected one",
            Self::PluginInstall => "Install a portable .rfplugin package",
            Self::PluginRemoval => "Remove an installed plugin and its data",
            Self::PluginResources => "Give a plugin a sound library or ROM it declares",
            Self::PluginWebSurfaces => "Show a plugin's own PLAY and CONFIG interfaces",
            Self::ControllerPackages => "Drive hardware surfaces from a .rfcontroller",
            Self::MidiInput => "Play from a connected MIDI controller",
            Self::MidiOutput => "Send MIDI to hardware",
            Self::MidiHotplug => "Notice controllers connecting while running",
            Self::VirtualMidi => "Play from an on-screen keyboard or pads",
            Self::AudioDeviceSelection => "Choose the audio device and buffer size",
            Self::PersistentStorage => "Keep instruments and performances between runs",
            Self::SessionRestore => "Restore the previous session on the next start",
            Self::OfflineOperation => "Work with no network connection",
            Self::RuntimeMetering => "Survive a plugin that stops responding",
        }
    }
}

impl fmt::Display for Capability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.id())
    }
}

/// What RackForge is, everywhere. A host that cannot do these is not a
/// RackForge host, so the tests refuse a declaration that gives them up.
pub const CORE: &[Capability] = &[
    Capability::PlayInstrument,
    Capability::SelectProgram,
    Capability::MasterLevelAndPan,
    Capability::VirtualMidi,
    Capability::PluginParameters,
    Capability::PerformanceLibrary,
    Capability::PersistentStorage,
];

/// A host's answer for one capability. There is no default: a new capability
/// forces every host to say where it stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Support {
    /// Implemented and expected to work.
    Yes,
    /// Not implemented yet. The reason says what is missing, so the gap is a
    /// known piece of work rather than an oversight.
    Planned(&'static str),
    /// Cannot be implemented on this host. The reason says why, so nobody
    /// re-opens it every year.
    Unavailable(&'static str),
    /// Nobody has checked. Honest about the audit rather than guessing, and
    /// visible in the document as an open question.
    Unaudited,
}

impl Support {
    pub const fn is_supported(self) -> bool {
        matches!(self, Self::Yes)
    }

    /// The document's cell for this answer.
    pub const fn mark(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::Planned(_) => "planned",
            Self::Unavailable(_) => "no",
            Self::Unaudited => "unaudited",
        }
    }

    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::Planned(reason) | Self::Unavailable(reason) => Some(reason),
            _ => None,
        }
    }
}

/// One RackForge host.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Host {
    Windows,
    Android,
    RaspberryPi,
    Browser,
}

impl Host {
    pub const ALL: &'static [Self] = &[
        Self::Windows,
        Self::Android,
        Self::RaspberryPi,
        Self::Browser,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Android => "Android",
            Self::RaspberryPi => "Raspberry Pi",
            Self::Browser => "Browser",
        }
    }

    /// Everything this host has declared.
    pub fn profile(self) -> &'static [(Capability, Support)] {
        match self {
            Self::Windows => WINDOWS,
            Self::Android => ANDROID,
            Self::RaspberryPi => RASPBERRY_PI,
            Self::Browser => BROWSER,
        }
    }

    /// This host's answer for one capability.
    pub fn support(self, capability: Capability) -> Option<Support> {
        self.profile()
            .iter()
            .find(|(declared, _)| *declared == capability)
            .map(|(_, support)| *support)
    }

    /// The capabilities this host implements.
    pub fn supported(self) -> Vec<Capability> {
        self.profile()
            .iter()
            .filter(|(_, support)| support.is_supported())
            .map(|(capability, _)| *capability)
            .collect()
    }
}

/// Reasons that recur, written once so they read the same in every row.
mod why {
    pub const NO_WEB_AUDIO_DEVICES: &str = "a page renders into the output the browser gives it and cannot enumerate or configure \
         audio hardware";
    pub const NO_BROWSER_METERING: &str = "the engine inside a page does not meter guest execution, so a plugin that stops \
         responding blocks the audio callback";
}

const WINDOWS: &[(Capability, Support)] = &[
    (Capability::PlayInstrument, Support::Yes),
    (Capability::SelectProgram, Support::Yes),
    (Capability::MasterLevelAndPan, Support::Yes),
    (Capability::VirtualMidi, Support::Yes),
    (Capability::MidiInput, Support::Yes),
    (Capability::MidiOutput, Support::Unaudited),
    (Capability::MidiHotplug, Support::Unaudited),
    (Capability::PluginParameters, Support::Yes),
    (Capability::HostPresets, Support::Yes),
    (Capability::ProgramDrafts, Support::Yes),
    (Capability::Auditions, Support::Yes),
    (Capability::PerformanceLibrary, Support::Yes),
    (Capability::RackRendering, Support::Unaudited),
    (Capability::RackSlotStateEditing, Support::Yes),
    (Capability::PluginInstall, Support::Yes),
    (Capability::PluginRemoval, Support::Yes),
    (Capability::PluginResources, Support::Yes),
    (Capability::PluginWebSurfaces, Support::Yes),
    (Capability::ControllerPackages, Support::Unaudited),
    (Capability::AudioDeviceSelection, Support::Yes),
    (Capability::PersistentStorage, Support::Yes),
    (Capability::SessionRestore, Support::Yes),
    (Capability::OfflineOperation, Support::Yes),
    (Capability::RuntimeMetering, Support::Yes),
];

const ANDROID: &[(Capability, Support)] = &[
    (Capability::PlayInstrument, Support::Yes),
    (Capability::SelectProgram, Support::Yes),
    (Capability::MasterLevelAndPan, Support::Yes),
    (Capability::VirtualMidi, Support::Yes),
    (Capability::MidiInput, Support::Yes),
    (Capability::MidiOutput, Support::Unaudited),
    (Capability::MidiHotplug, Support::Unaudited),
    (Capability::PluginParameters, Support::Yes),
    (Capability::HostPresets, Support::Yes),
    (Capability::ProgramDrafts, Support::Unaudited),
    (Capability::Auditions, Support::Unaudited),
    (Capability::PerformanceLibrary, Support::Yes),
    (Capability::RackRendering, Support::Unaudited),
    (Capability::RackSlotStateEditing, Support::Unaudited),
    (Capability::PluginInstall, Support::Yes),
    (Capability::PluginRemoval, Support::Yes),
    (Capability::PluginResources, Support::Yes),
    (Capability::PluginWebSurfaces, Support::Yes),
    (Capability::ControllerPackages, Support::Unaudited),
    (Capability::AudioDeviceSelection, Support::Yes),
    (Capability::PersistentStorage, Support::Yes),
    (Capability::SessionRestore, Support::Yes),
    (Capability::OfflineOperation, Support::Yes),
    (Capability::RuntimeMetering, Support::Yes),
];

const RASPBERRY_PI: &[(Capability, Support)] = &[
    (Capability::PlayInstrument, Support::Yes),
    (Capability::SelectProgram, Support::Yes),
    (Capability::MasterLevelAndPan, Support::Yes),
    (Capability::VirtualMidi, Support::Yes),
    (Capability::MidiInput, Support::Yes),
    (Capability::MidiOutput, Support::Yes),
    (Capability::MidiHotplug, Support::Yes),
    (Capability::PluginParameters, Support::Yes),
    (Capability::HostPresets, Support::Yes),
    (Capability::ProgramDrafts, Support::Yes),
    (Capability::Auditions, Support::Yes),
    (Capability::PerformanceLibrary, Support::Yes),
    (Capability::RackRendering, Support::Yes),
    (Capability::RackSlotStateEditing, Support::Yes),
    (Capability::PluginInstall, Support::Yes),
    (Capability::PluginRemoval, Support::Yes),
    (Capability::PluginResources, Support::Yes),
    (Capability::PluginWebSurfaces, Support::Yes),
    (Capability::ControllerPackages, Support::Yes),
    (Capability::AudioDeviceSelection, Support::Yes),
    (Capability::PersistentStorage, Support::Yes),
    (Capability::SessionRestore, Support::Yes),
    (Capability::OfflineOperation, Support::Yes),
    (Capability::RuntimeMetering, Support::Yes),
];

const BROWSER: &[(Capability, Support)] = &[
    (Capability::PlayInstrument, Support::Yes),
    (Capability::SelectProgram, Support::Yes),
    (Capability::MasterLevelAndPan, Support::Yes),
    (Capability::VirtualMidi, Support::Yes),
    (Capability::MidiInput, Support::Yes),
    (Capability::MidiOutput, Support::Unaudited),
    (Capability::MidiHotplug, Support::Yes),
    (Capability::PluginParameters, Support::Yes),
    (Capability::HostPresets, Support::Yes),
    (
        Capability::ProgramDrafts,
        Support::Planned("the program-draft commands are not implemented in the browser host"),
    ),
    (
        Capability::Auditions,
        Support::Planned("audition leases are not implemented in the browser host"),
    ),
    (Capability::PerformanceLibrary, Support::Yes),
    (
        Capability::RackRendering,
        Support::Planned(
            "the page renders the active PLAY instrument; Rack slots are not mixed yet",
        ),
    ),
    (
        Capability::RackSlotStateEditing,
        Support::Planned("isolated plugin state is not exposed by the browser host yet"),
    ),
    (Capability::PluginInstall, Support::Yes),
    (Capability::PluginRemoval, Support::Yes),
    (
        Capability::PluginResources,
        Support::Planned(
            "the host installs a chosen file into a plugin's private storage and reloads it, but \
             no packaged plugin here asks for one, so the path a plugin's own interface takes is \
             unproven",
        ),
    ),
    (Capability::PluginWebSurfaces, Support::Yes),
    (Capability::ControllerPackages, Support::Unaudited),
    (
        Capability::AudioDeviceSelection,
        Support::Unavailable(why::NO_WEB_AUDIO_DEVICES),
    ),
    (Capability::PersistentStorage, Support::Yes),
    (Capability::SessionRestore, Support::Yes),
    (Capability::OfflineOperation, Support::Yes),
    (
        Capability::RuntimeMetering,
        Support::Unavailable(why::NO_BROWSER_METERING),
    ),
];

/// Renders the parity document from this declaration.
///
/// `docs/PLATFORM_PARITY.md` is this function's output. A test compares them,
/// so the document is a view of the record rather than a second copy of it.
pub fn parity_document() -> String {
    let mut out = String::new();
    out.push_str("# Platform parity\n\n");
    out.push_str(
        "RackForge runs the same instruments and performances on every host it supports. This\n\
         table is where that promise is kept honest: it is generated from\n\
         `crates/rackforge-host-capabilities`, and a test fails if the two disagree, so it\n\
         cannot go stale. Do not edit it by hand — change the declaration and run\n\
         `cargo test -p rackforge-host-capabilities`.\n\n",
    );
    out.push_str(
        "`yes` means implemented, `planned` means a known gap with a reason below, `no` means it\n\
         cannot exist on that host, and `unaudited` means nobody has checked yet.\n\n",
    );

    out.push_str("| Capability |");
    for host in Host::ALL {
        out.push_str(&format!(" {} |", host.name()));
    }
    out.push_str("\n| --- |");
    for _ in Host::ALL {
        out.push_str(" --- |");
    }
    out.push('\n');
    for capability in Capability::ALL {
        out.push_str(&format!("| {} |", capability.summary()));
        for host in Host::ALL {
            let support = host.support(*capability).expect("every cell is declared");
            out.push_str(&format!(" {} |", support.mark()));
        }
        out.push('\n');
    }

    out.push_str("\n## Why a capability is missing\n\n");
    for host in Host::ALL {
        let reasons: Vec<_> = Capability::ALL
            .iter()
            .filter_map(|capability| {
                let support = host.support(*capability)?;
                support
                    .reason()
                    .map(|reason| (capability.summary(), support.mark(), reason))
            })
            .collect();
        if reasons.is_empty() {
            continue;
        }
        out.push_str(&format!("### {}\n\n", host.name()));
        for (summary, mark, reason) in reasons {
            out.push_str(&format!("- **{summary}** ({mark}): {reason}\n"));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_host_answers_for_every_capability() {
        for host in Host::ALL {
            let declared: BTreeSet<_> = host
                .profile()
                .iter()
                .map(|(capability, _)| *capability)
                .collect();
            for capability in Capability::ALL {
                assert!(
                    declared.contains(capability),
                    "{} does not say where it stands on {capability}",
                    host.name(),
                );
            }
            assert_eq!(
                declared.len(),
                host.profile().len(),
                "{} declares {capability_word} twice",
                host.name(),
                capability_word = "a capability",
            );
        }
    }

    #[test]
    fn no_capability_is_declared_without_being_listed() {
        let known: BTreeSet<_> = Capability::ALL.iter().copied().collect();
        for host in Host::ALL {
            for (capability, _) in host.profile() {
                assert!(
                    known.contains(capability),
                    "{} declares {capability}, which Capability::ALL does not list",
                    host.name(),
                );
            }
        }
    }

    #[test]
    fn every_host_keeps_the_core_promise() {
        for host in Host::ALL {
            for capability in CORE {
                let support = host.support(*capability).expect("declared");
                assert!(
                    support.is_supported(),
                    "{} gives up {capability}, which every RackForge host must have",
                    host.name(),
                );
            }
        }
    }

    #[test]
    fn a_gap_always_carries_its_reason() {
        for host in Host::ALL {
            for (capability, support) in host.profile() {
                if let Support::Planned(reason) | Support::Unavailable(reason) = support {
                    assert!(
                        reason.len() > 20,
                        "{}'s reason for {capability} does not explain anything",
                        host.name(),
                    );
                }
            }
        }
    }

    #[test]
    fn the_parity_document_matches_this_declaration() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/PLATFORM_PARITY.md");
        let expected = parity_document();
        if std::env::var("UPDATE_PARITY").is_ok() {
            std::fs::write(path, &expected).expect("writing the parity document");
            return;
        }
        let actual = std::fs::read_to_string(path).expect("reading docs/PLATFORM_PARITY.md");
        assert_eq!(
            actual, expected,
            "docs/PLATFORM_PARITY.md is out of date; run \
             UPDATE_PARITY=1 cargo test -p rackforge-host-capabilities",
        );
    }
}
