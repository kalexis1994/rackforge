//! Device identity and recovery for MIDI inputs that disappear and come back.
//!
//! A kicked USB cable is the most likely physical failure during a performance,
//! and it is a silent one: the ALSA port vanishes, the callback simply stops
//! being called, and the process keeps running perfectly. `Restart=always` does
//! not help, because nothing crashed. Without a supervisor the instrument goes
//! mute until someone restarts the service, which is not something that can be
//! done between two bars.
//!
//! Recovery has two halves, and the second one is the musical half:
//!
//! 1. Re-enumerate periodically and reconnect a device by its *stable* identity,
//!    so the reconnected keyboard keeps the [`MidiSourceKey`] its routes were
//!    compiled against.
//! 2. Release the notes the vanished device can no longer release itself. A key
//!    held at the moment the cable left never sends its Note Off, so without
//!    this the instrument reconnects underneath a note that drones forever.
//!
//! The panic is injected into the ordinary ingress channel rather than applied
//! to the engine directly. It therefore travels the same routing, transform and
//! plugin path as a Note Off the player actually sent, and the audio thread
//! needs no knowledge that any of this happened.

use rackforge_midi_api::{MidiPacket, MidiSourceId, MidiSourceKey};

/// MIDI channels a panic has to cover.
const MIDI_CHANNELS: u8 = 16;

/// Control change that releases the sustain pedal.
const CC_SUSTAIN: u8 = 64;

/// Control change that releases every sounding note.
const CC_ALL_NOTES_OFF: u8 = 123;

/// A device the supervisor keeps connected for the life of the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SupervisedSource {
    /// Routing key. Stable across reconnection by construction.
    pub key: MidiSourceKey,
    /// Identity derived from the port name with its ephemeral address removed.
    pub id: MidiSourceId,
    /// Whether the supervisor currently holds an open connection.
    pub connected: bool,
}

/// Work the supervisor must perform to match reality.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisorAction {
    /// The device is present but not connected.
    Connect(MidiSourceKey),
    /// The device vanished while connected; drop it and release its notes.
    Release(MidiSourceKey),
}

/// Compares the supervised set against the devices currently enumerated.
///
/// Only devices already known to the registry are acted on. A keyboard that
/// appears mid-session cannot be adopted here because the compiled routes were
/// built against the registry at startup, and inventing a key for it would
/// route its notes nowhere. Reconnecting a *known* device is precisely the case
/// where the original key still applies.
pub fn reconcile(
    supervised: &[SupervisedSource],
    present: &[MidiSourceId],
) -> Vec<SupervisorAction> {
    let mut actions = Vec::new();
    for source in supervised {
        let visible = present.contains(&source.id);
        match (source.connected, visible) {
            (false, true) => actions.push(SupervisorAction::Connect(source.key)),
            (true, false) => actions.push(SupervisorAction::Release(source.key)),
            _ => {}
        }
    }
    actions
}

/// How wide a net the disconnect panic casts.
///
/// Spraying all sixteen channels is the safe default in the abstract, but the
/// compiled route remaps every one of them onto the plugin's own channel, so
/// the engine receives sixteen *duplicate* all-notes-off in a single block.
/// That is a burst no player can produce, and it is worth being able to rule
/// in or out when an instrument misbehaves right after a reconnection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanicScope {
    /// Every MIDI channel.
    AllChannels,
    /// Channel 1 only, which is where a remapped route lands anyway.
    FirstChannelOnly,
    /// Send nothing. Held notes will drone; for diagnosis only.
    Disabled,
}

impl PanicScope {
    /// Reads the scope from `RACKFORGE_MIDI_PANIC`.
    ///
    /// Present so a suspected panic-induced fault can be bisected on the
    /// device without rebuilding, which matters when the only instrument that
    /// reproduces it is the one on stage.
    pub fn from_env() -> Self {
        match std::env::var("RACKFORGE_MIDI_PANIC").as_deref() {
            Ok("off") => Self::Disabled,
            Ok("first") => Self::FirstChannelOnly,
            _ => Self::AllChannels,
        }
    }
}

/// The messages a vanished device can no longer send on its own behalf.
///
/// Sustain is released *before* all-notes-off on each channel. The reverse
/// order is a silent bug: a damper pedal that was down when the cable left
/// holds the notes through the all-notes-off and they keep ringing.
pub fn panic_packets(scope: PanicScope) -> Vec<MidiPacket> {
    let channels = match scope {
        PanicScope::Disabled => return Vec::new(),
        PanicScope::FirstChannelOnly => 1,
        PanicScope::AllChannels => MIDI_CHANNELS,
    };
    let mut packets = Vec::with_capacity(usize::from(channels) * 2);
    for channel in 0..channels {
        let status = 0xB0 | channel;
        for controller in [CC_SUSTAIN, CC_ALL_NOTES_OFF] {
            if let Ok(packet) = MidiPacket::new(0, &[status, controller, 0]) {
                packets.push(packet);
            }
        }
    }
    packets
}

/// Derives an identity that survives replugging.
///
/// ALSA appends an ephemeral `client:port` address to each name, and that
/// address changes every time a device is plugged in. Stripping it is what
/// makes a reconnected keyboard the same source as the one that left, rather
/// than a new device whose notes have nowhere to go.
pub fn stable_alsa_source_id(name: &str) -> Result<MidiSourceId, anyhow::Error> {
    let stable_name = name
        .rsplit_once(' ')
        .filter(|(_, suffix)| {
            suffix.split_once(':').is_some_and(|(client, port)| {
                !client.is_empty()
                    && !port.is_empty()
                    && client.bytes().all(|byte| byte.is_ascii_digit())
                    && port.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        .map_or(name, |(stable, _)| stable);
    let mut slug = String::with_capacity(stable_name.len());
    let mut separator = false;
    for byte in stable_name.bytes() {
        if byte.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(char::from(byte.to_ascii_lowercase()));
            separator = false;
        } else {
            separator = true;
        }
    }
    if slug.is_empty() {
        slug.push_str("unnamed");
    }
    let hash = stable_name
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    MidiSourceId::new(format!("alsa.{slug}-{hash:016x}")).map_err(Into::into)
}

/// Whether a port name is a keyboard RackForge should play from.
///
/// Excludes the loopback, the DIN pass-through and the control-surface ports a
/// controller also exposes: those carry surface traffic, not performance notes.
pub fn is_performance_midi_input(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    folded.contains("midi")
        && !folded.contains("midi through")
        && !folded.contains("dinthru")
        && !folded.contains("mcu")
        && !folded.contains("hui")
        && !folded.contains(" alv")
        && !folded.contains("rackforge")
}

#[cfg(target_os = "linux")]
pub use supervisor::{DEFAULT_POLL_INTERVAL, spawn};

#[cfg(target_os = "linux")]
mod supervisor {
    use super::{
        PanicScope, SupervisedSource, SupervisorAction, is_performance_midi_input, panic_packets,
        reconcile, stable_alsa_source_id,
    };
    use anyhow::{Context, Result};
    use midir::{Ignore, MidiInput, MidiInputConnection};
    use rackforge_midi_api::{IngressMidiEvent, MidiSourceKey};
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;
    use std::sync::mpsc::SyncSender;
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    /// How often the port list is re-read.
    ///
    /// A second is well inside the gap between a cable being re-seated and the
    /// next entry, while leaving the scan far too infrequent to matter for CPU.
    /// It never runs on the audio thread regardless.
    pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);

    /// Starts the supervisor and returns once it owns the sources.
    ///
    /// The thread performs its first reconciliation immediately rather than
    /// after a poll interval, so startup connects without an artificial delay.
    pub fn spawn(
        sender: SyncSender<IngressMidiEvent>,
        observer: Option<SyncSender<IngressMidiEvent>>,
        connected_sources: Arc<Mutex<BTreeSet<u32>>>,
        mut supervised: Vec<SupervisedSource>,
        interval: Duration,
    ) -> Result<JoinHandle<()>> {
        // Resolved once, off the reconnection path, so a diagnosis run cannot
        // change behaviour halfway through a performance.
        let scope = PanicScope::from_env();
        println!(
            "MIDI_SUPERVISOR_READY panic_scope={scope:?} interval_ms={}",
            interval.as_millis()
        );
        thread::Builder::new()
            .name("rackforge-midi-supervisor".into())
            .spawn(move || {
                let mut connections: BTreeMap<u32, MidiInputConnection<()>> = BTreeMap::new();
                loop {
                    match present_source_ids() {
                        Ok(present) => {
                            for action in reconcile(&supervised, &present) {
                                apply(
                                    action,
                                    &mut supervised,
                                    &mut connections,
                                    &sender,
                                    observer.as_ref(),
                                    &connected_sources,
                                    scope,
                                );
                            }
                        }
                        // A failed scan must not end the supervisor: the ALSA
                        // sequencer can be transiently unavailable while a
                        // device re-enumerates, which is exactly the moment
                        // this thread exists to survive.
                        Err(error) => {
                            eprintln!("MIDI_SUPERVISOR_SCAN_FAILED error={error:#}");
                        }
                    }
                    thread::sleep(interval);
                }
            })
            .context("spawning RackForge MIDI supervisor")
    }

    fn apply(
        action: SupervisorAction,
        supervised: &mut [SupervisedSource],
        connections: &mut BTreeMap<u32, MidiInputConnection<()>>,
        sender: &SyncSender<IngressMidiEvent>,
        observer: Option<&SyncSender<IngressMidiEvent>>,
        connected_sources: &Arc<Mutex<BTreeSet<u32>>>,
        scope: PanicScope,
    ) {
        match action {
            SupervisorAction::Connect(key) => {
                let Some(source) = supervised.iter_mut().find(|source| source.key == key) else {
                    return;
                };
                match connect(key, &source.id, sender.clone(), observer.cloned()) {
                    Ok(connection) => {
                        connections.insert(key.get(), connection);
                        source.connected = true;
                        if let Ok(mut connected) = connected_sources.lock() {
                            connected.insert(key.get());
                        }
                        println!(
                            "MIDI_SOURCE_CONNECTED source={} id={}",
                            key.get(),
                            source.id
                        );
                    }
                    Err(error) => {
                        // Left disconnected on purpose: the next scan retries.
                        // A device can be visible in the port list a moment
                        // before it accepts a connection.
                        eprintln!(
                            "MIDI_SOURCE_CONNECT_FAILED source={} id={} error={error:#}",
                            key.get(),
                            source.id
                        );
                    }
                }
            }
            SupervisorAction::Release(key) => {
                connections.remove(&key.get());
                if let Ok(mut connected) = connected_sources.lock() {
                    connected.remove(&key.get());
                }
                if let Some(source) = supervised.iter_mut().find(|source| source.key == key) {
                    source.connected = false;
                    eprintln!("MIDI_SOURCE_LOST source={} id={}", key.get(), source.id);
                }
                release_held_notes(key, sender, scope);
            }
        }
    }

    /// Injects the panic through the ordinary ingress path.
    ///
    /// Uses a blocking send: dropping these is not an option, because a lost
    /// packet here is a note that never stops. The queue drains at audio rate,
    /// so the bound is a bounded wait, not a deadlock.
    fn release_held_notes(
        source: MidiSourceKey,
        sender: &SyncSender<IngressMidiEvent>,
        scope: PanicScope,
    ) {
        let packets = panic_packets(scope);
        let count = packets.len();
        for packet in packets {
            if sender.send(IngressMidiEvent { source, packet }).is_err() {
                // The audio loop is gone; there is nothing left to silence.
                return;
            }
        }
        println!(
            "MIDI_SOURCE_PANIC_SENT source={} packets={count} scope={scope:?}",
            source.get()
        );
    }

    fn present_source_ids() -> Result<Vec<rackforge_midi_api::MidiSourceId>> {
        let scan = MidiInput::new("rackforge-core-scan")?;
        let mut present = Vec::new();
        for port in scan.ports() {
            let name = scan.port_name(&port)?;
            if !is_performance_midi_input(&name) {
                continue;
            }
            let id = stable_alsa_source_id(&name)?;
            if !present.contains(&id) {
                present.push(id);
            }
        }
        Ok(present)
    }

    fn connect(
        key: MidiSourceKey,
        id: &rackforge_midi_api::MidiSourceId,
        sender: SyncSender<IngressMidiEvent>,
        observer: Option<SyncSender<IngressMidiEvent>>,
    ) -> Result<MidiInputConnection<()>> {
        let mut midi = MidiInput::new(&format!("rackforge-core-live-{}", key.get()))?;
        midi.ignore(Ignore::None);
        // Matched by identity, never by name: the name carries the ephemeral
        // ALSA address, which is different after every replug.
        let port = midi
            .ports()
            .into_iter()
            .find(|port| {
                midi.port_name(port)
                    .ok()
                    .filter(|name| is_performance_midi_input(name))
                    .and_then(|name| stable_alsa_source_id(&name).ok())
                    .is_some_and(|candidate| &candidate == id)
            })
            .with_context(|| format!("MIDI input {id} disappeared during connection"))?;
        midi.connect(
            &port,
            &format!("rackforge-core-input-{}", key.get()),
            move |_timestamp, message, _| {
                if let Ok(packet) = rackforge_midi_api::MidiPacket::new(0, message) {
                    let ingress = IngressMidiEvent {
                        source: key,
                        packet,
                    };
                    if let Some(observer) = &observer {
                        let _ = observer.try_send(ingress);
                    }
                    let _ = sender.try_send(ingress);
                }
            },
            (),
        )
        .map_err(|error| anyhow::anyhow!("connecting MIDI input {id}: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(key: u32, id: &str, connected: bool) -> SupervisedSource {
        SupervisedSource {
            key: MidiSourceKey::new(key),
            id: MidiSourceId::new(id.to_string()).unwrap(),
            connected,
        }
    }

    fn id(value: &str) -> MidiSourceId {
        MidiSourceId::new(value.to_string()).unwrap()
    }

    #[test]
    fn a_steady_state_produces_no_work() {
        let supervised = vec![source(0, "alsa.keylab", true)];
        assert!(reconcile(&supervised, &[id("alsa.keylab")]).is_empty());
    }

    #[test]
    fn a_vanished_device_is_released() {
        let supervised = vec![source(0, "alsa.keylab", true)];
        assert_eq!(
            reconcile(&supervised, &[]),
            vec![SupervisorAction::Release(MidiSourceKey::new(0))]
        );
    }

    #[test]
    fn a_returning_device_reconnects_under_its_original_key() {
        let supervised = vec![source(7, "alsa.keylab", false)];
        assert_eq!(
            reconcile(&supervised, &[id("alsa.keylab")]),
            vec![SupervisorAction::Connect(MidiSourceKey::new(7))]
        );
    }

    #[test]
    fn a_device_that_stays_absent_is_released_only_once() {
        let supervised = vec![source(0, "alsa.keylab", false)];
        assert!(reconcile(&supervised, &[]).is_empty());
    }

    #[test]
    fn an_unknown_device_is_never_adopted() {
        let supervised = vec![source(0, "alsa.keylab", true)];
        let actions = reconcile(&supervised, &[id("alsa.keylab"), id("alsa.some-other-pad")]);
        assert!(
            actions.is_empty(),
            "a device the routes were not compiled against must not be connected"
        );
    }

    #[test]
    fn each_device_is_reconciled_independently() {
        let supervised = vec![
            source(0, "alsa.keylab", true),
            source(1, "alsa.pads", false),
        ];
        assert_eq!(
            reconcile(&supervised, &[id("alsa.pads")]),
            vec![
                SupervisorAction::Release(MidiSourceKey::new(0)),
                SupervisorAction::Connect(MidiSourceKey::new(1)),
            ]
        );
    }

    #[test]
    fn a_disabled_panic_sends_nothing() {
        assert!(panic_packets(PanicScope::Disabled).is_empty());
    }

    #[test]
    fn a_narrowed_panic_covers_only_the_first_channel() {
        let packets = panic_packets(PanicScope::FirstChannelOnly);
        assert_eq!(packets.len(), 2);
        assert!(packets.iter().all(|packet| packet.data[0] == 0xB0));
    }

    #[test]
    fn the_scope_defaults_to_every_channel_for_an_unknown_value() {
        // Safety property: a typo in the diagnostic variable must not silently
        // leave held notes ringing.
        unsafe { std::env::set_var("RACKFORGE_MIDI_PANIC", "nonsense") };
        assert_eq!(PanicScope::from_env(), PanicScope::AllChannels);
        unsafe { std::env::remove_var("RACKFORGE_MIDI_PANIC") };
        assert_eq!(PanicScope::from_env(), PanicScope::AllChannels);
    }

    #[test]
    fn the_panic_covers_every_channel() {
        let packets = panic_packets(PanicScope::AllChannels);
        assert_eq!(packets.len(), usize::from(MIDI_CHANNELS) * 2);
        for channel in 0..MIDI_CHANNELS {
            assert!(
                packets
                    .iter()
                    .any(|packet| packet.data[0] == (0xB0 | channel)
                        && packet.data[1] == CC_ALL_NOTES_OFF),
                "channel {channel} was left sounding"
            );
        }
    }

    #[test]
    fn sustain_is_released_before_the_notes_on_every_channel() {
        let packets = panic_packets(PanicScope::AllChannels);
        for channel in 0..MIDI_CHANNELS {
            let status = 0xB0 | channel;
            let sustain = packets
                .iter()
                .position(|packet| packet.data[0] == status && packet.data[1] == CC_SUSTAIN)
                .expect("sustain release is missing");
            let notes = packets
                .iter()
                .position(|packet| packet.data[0] == status && packet.data[1] == CC_ALL_NOTES_OFF)
                .expect("all-notes-off is missing");
            assert!(
                sustain < notes,
                "channel {channel} releases its notes while the damper is still down"
            );
        }
    }

    #[test]
    fn the_panic_releases_rather_than_holds() {
        for packet in panic_packets(PanicScope::AllChannels) {
            assert_eq!(packet.data[2], 0, "a panic message carried a nonzero value");
        }
    }

    #[test]
    fn stable_identity_ignores_the_ephemeral_client_port() {
        let first =
            stable_alsa_source_id("KL Essential 61 mk3:KL Essential 61 mk3 MIDI 24:0").unwrap();
        let second =
            stable_alsa_source_id("KL Essential 61 mk3:KL Essential 61 mk3 MIDI 28:0").unwrap();
        assert_eq!(
            first, second,
            "a replugged keyboard must resolve to the same source"
        );
        assert!(first.as_str().starts_with("alsa.kl-essential-61-mk3"));
    }

    #[test]
    fn distinct_devices_keep_distinct_identities() {
        let keylab = stable_alsa_source_id("KL Essential 61 mk3 MIDI 24:0").unwrap();
        let other = stable_alsa_source_id("Some Other Keyboard MIDI 24:0").unwrap();
        assert_ne!(keylab, other);
    }

    #[test]
    fn performance_inputs_exclude_surface_and_loopback_ports() {
        assert!(is_performance_midi_input("KL Essential 61 mk3 MIDI 28:0"));
        assert!(is_performance_midi_input("Unknown USB MIDI 31:0"));
        assert!(!is_performance_midi_input(
            "KL Essential 61 mk3 DINTHRU 28:1"
        ));
        assert!(!is_performance_midi_input(
            "KL Essential 61 mk3 MCU/HUI 28:2"
        ));
        assert!(!is_performance_midi_input("KL Essential 61 mk3 ALV 28:3"));
        assert!(!is_performance_midi_input("Midi Through MIDI 0:1"));
    }
}
