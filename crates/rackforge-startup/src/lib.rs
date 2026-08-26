//! Shared startup phases and readiness reporting.
//!
//! RackForge hosts differ in how they are launched, but they share one
//! availability policy: make the current instrument audible first, connect
//! control surfaces second, and leave catalog/network work for the background.
//! This crate gives every host the same vocabulary and rejects accidental
//! phase regressions without pulling the full audio engine into small hosts.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// Increasing availability milestones. Hosts may skip an inapplicable phase,
/// but may never move backwards.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum StartupPhase {
    /// The selected audio graph can render through the chosen output.
    AudioReady = 1,
    /// Musical input and host-owned control surfaces have been published.
    ControlReady = 2,
    /// Non-critical discovery, Web and network work has been released.
    BackgroundReady = 3,
}

impl StartupPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AudioReady => "audio_ready",
            Self::ControlReady => "control_ready",
            Self::BackgroundReady => "background_ready",
        }
    }
}

impl fmt::Display for StartupPhase {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

struct StartupTimelineInner {
    host: String,
    started: Instant,
    highest: AtomicU8,
}

/// Thread-safe monotonic startup telemetry for one host generation.
#[derive(Clone)]
pub struct StartupTimeline(Arc<StartupTimelineInner>);

impl StartupTimeline {
    pub fn new(host: impl Into<String>) -> Self {
        let timeline = Self(Arc::new(StartupTimelineInner {
            host: host.into(),
            started: Instant::now(),
            highest: AtomicU8::new(0),
        }));
        println!(
            "STARTUP_PHASE host={} phase=starting elapsed_ms=0",
            timeline.0.host
        );
        timeline
    }

    pub fn host(&self) -> &str {
        &self.0.host
    }

    pub fn elapsed(&self) -> Duration {
        self.0.started.elapsed()
    }

    /// Publishes `phase` once. Repeating the current phase is idempotent;
    /// attempting to regress is an error so platform startup cannot silently
    /// reintroduce a dependency on lower-priority work.
    pub fn advance(&self, phase: StartupPhase) -> Result<Duration, StartupPhaseRegression> {
        let requested = phase as u8;
        loop {
            let current = self.0.highest.load(Ordering::Acquire);
            if requested < current {
                return Err(StartupPhaseRegression {
                    current: phase_from_rank(current),
                    requested: phase,
                });
            }
            if requested == current {
                return Ok(self.elapsed());
            }
            if self
                .0
                .highest
                .compare_exchange(current, requested, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let elapsed = self.elapsed();
                println!(
                    "STARTUP_PHASE host={} phase={} elapsed_ms={}",
                    self.0.host,
                    phase,
                    elapsed.as_millis()
                );
                return Ok(elapsed);
            }
        }
    }

    pub fn highest(&self) -> Option<StartupPhase> {
        let rank = self.0.highest.load(Ordering::Acquire);
        (rank != 0).then(|| phase_from_rank(rank))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StartupPhaseRegression {
    pub current: StartupPhase,
    pub requested: StartupPhase,
}

impl fmt::Display for StartupPhaseRegression {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "startup phase cannot move from {} back to {}",
            self.current, self.requested
        )
    }
}

impl std::error::Error for StartupPhaseRegression {}

fn phase_from_rank(rank: u8) -> StartupPhase {
    match rank {
        1 => StartupPhase::AudioReady,
        2 => StartupPhase::ControlReady,
        3 => StartupPhase::BackgroundReady,
        _ => unreachable!("startup phase rank is written only from StartupPhase"),
    }
}

/// Tells a systemd `Type=notify` unit that its critical startup work is done.
/// Other platforms safely receive `Ok(false)`.
#[cfg(target_os = "linux")]
pub fn notify_service_ready(status: &str) -> std::io::Result<bool> {
    let Some(endpoint) = std::env::var_os("NOTIFY_SOCKET") else {
        return Ok(false);
    };
    send_systemd_notification(&endpoint, &format!("READY=1\nSTATUS={status}"))?;
    Ok(true)
}

#[cfg(not(target_os = "linux"))]
pub fn notify_service_ready(_status: &str) -> std::io::Result<bool> {
    Ok(false)
}

#[cfg(target_os = "linux")]
fn send_systemd_notification(endpoint: &std::ffi::OsStr, message: &str) -> std::io::Result<()> {
    use std::os::linux::net::SocketAddrExt;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::net::{SocketAddr, UnixDatagram};

    let bytes = endpoint.as_bytes();
    let address = if let Some(abstract_name) = bytes.strip_prefix(b"@") {
        SocketAddr::from_abstract_name(abstract_name)?
    } else {
        SocketAddr::from_pathname(endpoint)?
    };
    let socket = UnixDatagram::unbound()?;
    socket.send_to_addr(message.as_bytes(), &address)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_are_monotonic_idempotent_and_may_skip_optional_work() {
        let timeline = StartupTimeline::new("test");
        timeline.advance(StartupPhase::AudioReady).unwrap();
        timeline.advance(StartupPhase::AudioReady).unwrap();
        timeline.advance(StartupPhase::BackgroundReady).unwrap();
        assert_eq!(timeline.highest(), Some(StartupPhase::BackgroundReady));
        assert_eq!(
            timeline.advance(StartupPhase::ControlReady).unwrap_err(),
            StartupPhaseRegression {
                current: StartupPhase::BackgroundReady,
                requested: StartupPhase::ControlReady,
            }
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn systemd_notification_uses_the_supplied_socket() {
        use std::os::unix::net::UnixDatagram;

        let path = std::env::temp_dir().join(format!(
            "rackforge-notify-test-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        let _ = std::fs::remove_file(&path);
        let receiver = UnixDatagram::bind(&path).unwrap();
        send_systemd_notification(path.as_os_str(), "READY=1\nSTATUS=audible").unwrap();
        let mut bytes = [0_u8; 128];
        let count = receiver.recv(&mut bytes).unwrap();
        assert_eq!(&bytes[..count], b"READY=1\nSTATUS=audible");
        drop(receiver);
        let _ = std::fs::remove_file(path);
    }
}
