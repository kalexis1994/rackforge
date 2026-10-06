//! The engine's health, as the control socket's `AudioHealth` reports it.
//!
//! The audio thread counts what went wrong and how long each period's work
//! took; the control side reads the counts. Everything is an atomic: no
//! allocation, lock or system call on the audio thread. Totals run from the
//! process's start and only grow, so a reader that wants an interval takes
//! the difference of two reads. The `recent_*` values and the load cover the
//! window since the previous `take`, which is what the interface's readout
//! polls.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rackforge_control_api::AudioHealthSnapshot;

#[derive(Default)]
pub struct EngineHealth {
    period_ns: AtomicU64,
    period_frames: AtomicU64,
    totals: Counters,
    window: Counters,
    window_work_ns: AtomicU64,
    window_peak_ns: AtomicU64,
    window_overrun_ns: AtomicU64,
    window_gap_ns: AtomicU64,
}

#[derive(Default)]
struct Counters {
    blocks: AtomicU64,
    overruns: AtomicU64,
    underruns: AtomicU64,
    capture_xruns: AtomicU64,
    stream_errors: AtomicU64,
    midi_dropped: AtomicU64,
}

impl Counters {
    fn add(&self, field: impl Fn(&Self) -> &AtomicU64, count: u64) {
        field(self).fetch_add(count, Ordering::Relaxed);
    }
}

impl EngineHealth {
    /// The period the work is measured against. Called when the device is
    /// opened or reconfigured.
    pub fn set_period(&self, frames: usize, sample_rate_hz: u32) {
        let rate = u64::from(sample_rate_hz.max(1));
        self.period_frames.store(frames as u64, Ordering::Relaxed);
        self.period_ns
            .store(frames as u64 * 1_000_000_000 / rate, Ordering::Relaxed);
    }

    /// One period's work: everything the audio thread did between handing
    /// the device one period and starting to hand it the next -- control
    /// commands, MIDI, rendering, mastering -- and the time since the
    /// previous period was handed over, when there was one.
    ///
    /// Work longer than a period is an overrun even when the device's
    /// buffer absorbed it: the engine fell behind, and only the buffer kept
    /// it from being heard.
    pub fn record_period(&self, work: Duration, gap: Option<Duration>) {
        let work_ns = u64::try_from(work.as_nanos()).unwrap_or(u64::MAX);
        let period_ns = self.period_ns.load(Ordering::Relaxed);
        for counters in [&self.totals, &self.window] {
            counters.add(|c| &c.blocks, 1);
        }
        self.window_work_ns.fetch_add(work_ns, Ordering::Relaxed);
        self.window_peak_ns.fetch_max(work_ns, Ordering::Relaxed);
        if period_ns > 0 && work_ns > period_ns {
            for counters in [&self.totals, &self.window] {
                counters.add(|c| &c.overruns, 1);
            }
            self.window_overrun_ns.fetch_add(work_ns, Ordering::Relaxed);
        }
        if let Some(gap) = gap {
            let gap_ns = u64::try_from(gap.as_nanos()).unwrap_or(u64::MAX);
            self.window_gap_ns.fetch_max(gap_ns, Ordering::Relaxed);
        }
    }

    /// Periods the output device played with nothing written for them.
    pub fn record_underruns(&self, count: u64) {
        self.count(|c| &c.underruns, count);
    }

    /// Periods the input device captured with nobody reading them.
    pub fn record_capture_xruns(&self, count: u64) {
        self.count(|c| &c.capture_xruns, count);
    }

    /// A device that failed outright rather than running late.
    pub fn record_stream_error(&self) {
        self.count(|c| &c.stream_errors, 1);
    }

    pub fn record_midi_dropped(&self, count: u64) {
        self.count(|c| &c.midi_dropped, count);
    }

    fn count(&self, field: impl Fn(&Counters) -> &AtomicU64 + Copy, count: u64) {
        if count > 0 {
            self.totals.add(field, count);
            self.window.add(field, count);
        }
    }

    /// The totals, and the window since the previous call, which this
    /// closes.
    pub fn take(&self) -> AudioHealthSnapshot {
        let total =
            |field: fn(&Counters) -> &AtomicU64| field(&self.totals).load(Ordering::Relaxed);
        let recent =
            |field: fn(&Counters) -> &AtomicU64| field(&self.window).swap(0, Ordering::Relaxed);
        let window_blocks = recent(|c| &c.blocks);
        let window_overruns = recent(|c| &c.overruns);
        let work_ns = self.window_work_ns.swap(0, Ordering::Relaxed);
        let peak_ns = self.window_peak_ns.swap(0, Ordering::Relaxed);
        let overrun_ns = self.window_overrun_ns.swap(0, Ordering::Relaxed);
        let gap_ns = self.window_gap_ns.swap(0, Ordering::Relaxed);
        let period_ns = self.period_ns.load(Ordering::Relaxed) as f64;
        let period_frames = self.period_frames.load(Ordering::Relaxed) as f64;
        let percent = |ns: f64| {
            if period_ns > 0.0 {
                ns * 100.0 / period_ns
            } else {
                0.0
            }
        };
        AudioHealthSnapshot {
            load_percent: if window_blocks > 0 {
                percent(work_ns as f64 / window_blocks as f64)
            } else {
                0.0
            },
            peak_percent: percent(peak_ns as f64),
            overruns: total(|c| &c.overruns),
            stream_errors: total(|c| &c.stream_errors),
            midi_dropped: total(|c| &c.midi_dropped),
            recent_overruns: window_overruns,
            overrun_average_percent: if window_overruns > 0 {
                percent(overrun_ns as f64 / window_overruns as f64)
            } else {
                0.0
            },
            // ALSA hands this engine whole periods only.
            overrun_average_frames: if window_overruns > 0 {
                period_frames
            } else {
                0.0
            },
            block_frames: period_frames,
            // A push loop has no callback to be late; what the desktop counts
            // as a late callback -- a period the device played with nothing
            // written for it -- is ALSA's underrun.
            late_callbacks: total(|c| &c.underruns),
            recent_late_callbacks: recent(|c| &c.underruns),
            worst_gap_percent: percent(gap_ns as f64),
            // A plugin whose process call fails is quarantined and logged;
            // in a Rack that silences one Slot, not the block, so it is not
            // counted as a silenced block here.
            silenced_blocks: 0,
            recent_silenced_blocks: 0,
            driver_overloads: 0,
            driver_resyncs: 0,
            driver_skipped_buffers: 0,
            recent_driver_dropouts: 0,
            capture_glitches: total(|c| &c.capture_xruns),
            recent_capture_glitches: recent(|c| &c.capture_xruns),
            midi_late_driver: 0,
            midi_late_queue: 0,
            recent_midi_late: 0,
            worst_midi_driver_delay_ms: 0.0,
            worst_midi_queue_delay_ms: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_keep_growing_and_the_window_closes_on_each_take() {
        let health = EngineHealth::default();
        health.set_period(256, 48_000);
        let period = Duration::from_nanos(5_333_333);
        health.record_period(period / 4, None);
        health.record_period(period * 2, Some(period));
        health.record_underruns(3);
        health.record_capture_xruns(1);

        let first = health.take();
        assert_eq!(first.overruns, 1);
        assert_eq!(first.recent_overruns, 1);
        assert_eq!(first.late_callbacks, 3);
        assert_eq!(first.recent_late_callbacks, 3);
        assert_eq!(first.capture_glitches, 1);
        assert_eq!(first.block_frames, 256.0);
        assert!((first.peak_percent - 200.0).abs() < 0.1);
        assert!((first.load_percent - 112.5).abs() < 0.1);
        assert!((first.worst_gap_percent - 100.0).abs() < 0.1);

        health.record_period(period / 2, None);
        let second = health.take();
        assert_eq!(second.overruns, 1, "a total never resets");
        assert_eq!(second.recent_overruns, 0);
        assert_eq!(second.late_callbacks, 3);
        assert_eq!(second.recent_late_callbacks, 0);
        assert!((second.load_percent - 50.0).abs() < 0.1);
    }

    #[test]
    fn work_within_the_period_is_not_an_overrun() {
        let health = EngineHealth::default();
        health.set_period(256, 48_000);
        health.record_period(Duration::from_millis(5), None);
        assert_eq!(health.take().overruns, 0);
    }
}
