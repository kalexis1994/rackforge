//! Watches for the audio output changing underneath a running engine.
//!
//! The engine binds one output when it starts and plays through it until it
//! stops. That is right for the stream and wrong for the appliance: a player
//! carries a Raspberry Pi to a stage and plugs an interface into it, and
//! until now the engine kept rendering into the board's own headphone jack
//! -- or refused to start at all -- because the device it resolved at boot is
//! the only device it ever looks at.
//!
//! Like the MIDI supervisor, this thread does not try to absorb the change.
//! Re-binding a live ALSA stream to a different device mid-performance is a
//! much larger promise than the one worth making here: systemd restarts the
//! engine in a second, and it resolves devices again on the way up. So this
//! watches, decides, says what it saw, and asks for that restart.
//!
//! It is deliberately hard to trigger. The engine leaves a working output
//! only when that output is gone, or when a better *kind* of connection
//! appears -- and no interface displaces another interface, so plugging a
//! second one in beside a working one does nothing at all.

use crate::audio::{discover_audio_devices, present_audio_device_ids};
use anyhow::{Context, Result};
use rackforge_audio_api::{
    AudioDeviceId, AudioOutputProfile, AudioTransport, OutputChange, assess,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Slow on purpose. This costs an ALSA enumeration, it competes with a
/// realtime render thread, and nothing here needs to be noticed in under a
/// second: a player who has just plugged an interface in is still reaching
/// for the cable.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Watches the inventory and asks for a restart when the binding goes stale.
pub fn spawn(
    current_id: AudioDeviceId,
    current_transport: AudioTransport,
    profile: AudioOutputProfile,
    interval: Duration,
) -> Result<JoinHandle<()>> {
    println!(
        "AUDIO_SUPERVISOR_READY output={current_id} transport={current_transport:?} \
         interval_ms={}",
        interval.as_millis()
    );
    thread::Builder::new()
        .name("rackforge-audio-supervisor".into())
        .spawn(move || {
            loop {
                thread::sleep(interval);
                match discover_audio_devices() {
                    Ok(devices) => {
                        match assess(&current_id, current_transport, &profile, &devices) {
                            // Missing from the probed inventory is not gone:
                            // the engine holds the device, and a device held
                            // on both sides -- playing and capturing -- cannot
                            // be probed at all. Only a device the card list no
                            // longer has was unplugged.
                            Some(OutputChange::Lost)
                                if present_audio_device_ids()
                                    .is_ok_and(|present| present.contains(&current_id)) => {}
                            Some(OutputChange::Lost) => {
                                println!(
                                    "AUDIO_OUTPUT_LOST id={current_id} \
                                     action=restart-to-rebind"
                                );
                                std::process::exit(0);
                            }
                            Some(OutputChange::Better { id, transport }) => {
                                println!(
                                    "AUDIO_OUTPUT_ARRIVED id={id} transport={transport:?} \
                                     replacing={current_id} replacing_transport={current_transport:?} \
                                     action=restart-to-adopt"
                                );
                                std::process::exit(0);
                            }
                            None => {}
                        }
                    }
                    // A scan can fail while a device re-enumerates, which is
                    // the moment this thread exists to survive. Ending the
                    // watcher there would leave the engine unsupervised for
                    // the rest of the performance.
                    Err(error) => {
                        eprintln!("AUDIO_SUPERVISOR_SCAN_FAILED error={error:#}");
                    }
                }
            }
        })
        .context("spawning RackForge audio supervisor")
}
