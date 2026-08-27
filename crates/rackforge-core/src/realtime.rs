//! Real-time scheduling and memory residency for the audio path.
//!
//! The audio loop must never wait for the ordinary scheduler and must never
//! take a page fault. Both properties require two independent halves: the
//! platform has to *grant* the limits, and this process has to *request* them.
//! Granting alone is silent and does nothing; requesting alone fails.
//!
//! Neither half is fatal. A host that cannot obtain real-time scheduling still
//! plays, it just becomes vulnerable to dropouts under load. Because that
//! failure is inaudible until the worst possible moment, [`engage`] never
//! returns an error and instead reports exactly which half is missing, so the
//! appliance audit can surface it before a performance rather than after.

use std::fmt;

/// Default `SCHED_FIFO` priority requested for the audio path.
///
/// Deliberately below `sched_get_priority_max` so that interrupt and watchdog
/// kernel threads can still preempt a runaway audio callback. A host pinned at
/// the top of the range converts one stuck buffer into an unresponsive board.
pub const DEFAULT_AUDIO_PRIORITY: i32 = 75;

/// Outcome of requesting `SCHED_FIFO` for the calling thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchedulingState {
    /// The thread runs at the given real-time priority.
    Realtime { priority: i32 },
    /// The platform granted its best available boost, which is not a POSIX
    /// real-time class: MMCSS "Pro Audio" or TIME_CRITICAL on Windows.
    Boosted { class: &'static str },
    /// The platform did not grant `RLIMIT_RTPRIO`; the thread stays on the
    /// ordinary scheduler. `granted` is the ceiling actually available.
    Denied { requested: i32, granted: i32 },
    /// The request failed for a reason other than a missing limit.
    Failed { requested: i32, errno: i32 },
    /// Not applicable on this platform.
    Unsupported,
}

/// Outcome of locking the process image into physical memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryState {
    /// Current and future pages are resident.
    Locked,
    /// The platform did not grant `RLIMIT_MEMLOCK`.
    Denied,
    /// The request failed for a reason other than a missing limit.
    Failed { errno: i32 },
    /// Not applicable on this platform.
    Unsupported,
}

/// Combined real-time posture of the audio path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeStatus {
    pub scheduling: SchedulingState,
    pub memory: MemoryState,
}

impl RealtimeStatus {
    /// True when the platform granted everything it can grant. On Linux
    /// that is `SCHED_FIFO` plus locked memory; on platforms where a half
    /// simply does not exist (memory locking on Windows), the absent half
    /// does not count against the audio path.
    pub fn is_fully_engaged(&self) -> bool {
        let scheduling = matches!(
            self.scheduling,
            SchedulingState::Realtime { .. } | SchedulingState::Boosted { .. }
        );
        let memory = matches!(self.memory, MemoryState::Locked | MemoryState::Unsupported);
        scheduling && memory
    }

    /// True when the host is playable but exposed to scheduler-induced dropouts.
    pub fn is_degraded(&self) -> bool {
        !self.is_fully_engaged()
    }

    /// Operator-facing remedy for the missing half, if any.
    ///
    /// Returned instead of logged so that the caller owns output policy and
    /// this module stays usable from a real-time context.
    pub fn remedy(&self) -> Option<&'static str> {
        match (&self.scheduling, &self.memory) {
            (SchedulingState::Realtime { .. }, MemoryState::Locked) => None,
            (SchedulingState::Denied { .. }, _) | (_, MemoryState::Denied) => Some(
                "grant LimitRTPRIO and LimitMEMLOCK to the audio unit, \
                 or install platforms/raspberry-pi/etc/security/limits.d/rackforge-audio.conf",
            ),
            _ => Some("inspect the audio unit's scheduling and memlock limits"),
        }
    }
}

impl fmt::Display for RealtimeStatus {
    /// One stable, greppable line. `optimize-appliance.sh audit` parses the
    /// `REALTIME_` prefix, so the shape of this line is part of the contract.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheduling = match &self.scheduling {
            SchedulingState::Realtime { priority } => format!("fifo:{priority}"),
            SchedulingState::Boosted { class } => format!("boosted:{class}"),
            SchedulingState::Denied { requested, granted } => {
                format!("denied:requested={requested},granted={granted}")
            }
            SchedulingState::Failed { requested, errno } => {
                format!("failed:requested={requested},errno={errno}")
            }
            SchedulingState::Unsupported => "unsupported".to_string(),
        };
        let memory = match &self.memory {
            MemoryState::Locked => "locked".to_string(),
            MemoryState::Denied => "denied".to_string(),
            MemoryState::Failed { errno } => format!("failed:errno={errno}"),
            MemoryState::Unsupported => "unsupported".to_string(),
        };
        let state = if self.is_fully_engaged() {
            "ENGAGED"
        } else {
            "DEGRADED"
        };
        write!(
            formatter,
            "REALTIME_{state} scheduling={scheduling} memory={memory}"
        )
    }
}

/// Requests real-time scheduling and memory residency for the calling thread.
///
/// Must be called *on the thread that runs the audio loop*: `SCHED_FIFO` is a
/// per-thread property, so engaging it from a supervisor thread protects the
/// wrong thread and produces a status that lies.
///
/// The requested priority is clamped to the platform ceiling rather than
/// rejected, so a conservative default keeps working on kernels whose maximum
/// is lower than expected.
#[cfg(target_os = "linux")]
pub fn engage(priority: i32) -> RealtimeStatus {
    RealtimeStatus {
        scheduling: engage_scheduling(priority),
        memory: engage_memory_residency(),
    }
}

/// Windows: the best available posture is the Multimedia Class Scheduler's
/// "Pro Audio" task (the mechanism every DAW uses for its audio threads),
/// with plain `TIME_CRITICAL` as the fallback. Memory locking has no
/// equivalent grant to request, so it reports as not applicable.
#[cfg(target_os = "windows")]
pub fn engage(priority: i32) -> RealtimeStatus {
    RealtimeStatus {
        scheduling: engage_mmcss(priority),
        memory: MemoryState::Unsupported,
    }
}

#[cfg(target_os = "windows")]
fn engage_mmcss(requested: i32) -> SchedulingState {
    #[link(name = "avrt")]
    unsafe extern "system" {
        fn AvSetMmThreadCharacteristicsW(
            task_name: *const u16,
            task_index: *mut u32,
        ) -> *mut core::ffi::c_void;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut core::ffi::c_void;
        fn SetThreadPriority(thread: *mut core::ffi::c_void, priority: i32) -> i32;
        fn GetLastError() -> u32;
    }
    const THREAD_PRIORITY_TIME_CRITICAL: i32 = 15;

    let task_name: Vec<u16> = "Pro Audio"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut task_index = 0_u32;
    // SAFETY: a valid NUL-terminated UTF-16 name and an out-parameter; the
    // returned handle intentionally lives for the thread's lifetime.
    let handle = unsafe { AvSetMmThreadCharacteristicsW(task_name.as_ptr(), &mut task_index) };
    if !handle.is_null() {
        return SchedulingState::Boosted { class: "pro-audio" };
    }
    // SAFETY: the pseudo-handle for the calling thread is always valid.
    if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) } != 0 {
        return SchedulingState::Boosted {
            class: "time-critical",
        };
    }
    SchedulingState::Failed {
        requested,
        // SAFETY: plain thread-local error read.
        errno: unsafe { GetLastError() } as i32,
    }
}

/// Android: apps may not take `SCHED_FIFO` (a rare device grants it, so it
/// is still attempted), but they may move their own audio threads into the
/// urgent-audio niceness class — the same boost `Process.setThreadPriority
/// (THREAD_PRIORITY_URGENT_AUDIO)` applies, and what keeps pool workers
/// from being starved under the system-boosted AAudio callback. Memory
/// locking has no meaningful grant inside an app sandbox.
#[cfg(target_os = "android")]
pub fn engage(priority: i32) -> RealtimeStatus {
    RealtimeStatus {
        scheduling: engage_android(priority),
        memory: MemoryState::Unsupported,
    }
}

#[cfg(target_os = "android")]
fn engage_android(requested: i32) -> SchedulingState {
    /// `android.os.Process.THREAD_PRIORITY_URGENT_AUDIO`.
    const ANDROID_PRIORITY_URGENT_AUDIO: i32 = -19;
    let parameters = libc::sched_param {
        sched_priority: requested,
    };
    // SAFETY: plain syscalls on the calling thread with a valid parameter
    // block; failures are reported, never fatal.
    unsafe {
        if libc::sched_setscheduler(0, libc::SCHED_FIFO, &parameters) == 0 {
            return SchedulingState::Realtime {
                priority: requested,
            };
        }
        if libc::setpriority(libc::PRIO_PROCESS, 0, ANDROID_PRIORITY_URGENT_AUDIO) == 0 {
            return SchedulingState::Boosted {
                class: "urgent-audio",
            };
        }
        SchedulingState::Failed {
            requested,
            errno: *libc::__errno(),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "android")))]
pub fn engage(_priority: i32) -> RealtimeStatus {
    RealtimeStatus {
        scheduling: SchedulingState::Unsupported,
        memory: MemoryState::Unsupported,
    }
}

#[cfg(target_os = "linux")]
fn engage_scheduling(priority: i32) -> SchedulingState {
    let ceiling = platform_priority_ceiling();
    let granted = rtprio_limit().min(ceiling);
    let requested = clamp_priority(priority, ceiling);

    if granted < 1 {
        return SchedulingState::Denied {
            requested,
            granted: 0,
        };
    }

    let effective = requested.min(granted);
    let parameters = libc::sched_param {
        sched_priority: effective,
    };
    // SAFETY: `parameters` is a fully initialized `sched_param` and pid 0
    // designates the calling thread.
    let result = unsafe { libc::sched_setscheduler(0, libc::SCHED_FIFO, &parameters) };
    if result == 0 {
        return SchedulingState::Realtime {
            priority: effective,
        };
    }

    let errno = last_errno();
    if errno == libc::EPERM {
        SchedulingState::Denied {
            requested,
            granted: effective,
        }
    } else {
        SchedulingState::Failed { requested, errno }
    }
}

#[cfg(target_os = "linux")]
fn engage_memory_residency() -> MemoryState {
    // SAFETY: `mlockall` takes only a flag word and touches no caller memory.
    let result = unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) };
    if result == 0 {
        return MemoryState::Locked;
    }
    let errno = last_errno();
    if errno == libc::EPERM || errno == libc::ENOMEM {
        MemoryState::Denied
    } else {
        MemoryState::Failed { errno }
    }
}

#[cfg(target_os = "linux")]
fn platform_priority_ceiling() -> i32 {
    // SAFETY: queries a constant for a known scheduling policy.
    let maximum = unsafe { libc::sched_get_priority_max(libc::SCHED_FIFO) };
    if maximum < 1 { 1 } else { maximum }
}

#[cfg(target_os = "linux")]
fn rtprio_limit() -> i32 {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a fully initialized `rlimit` owned by this frame.
    let result = unsafe { libc::getrlimit(libc::RLIMIT_RTPRIO, &mut limit) };
    if result != 0 {
        return 0;
    }
    if limit.rlim_cur == libc::RLIM_INFINITY {
        return i32::MAX;
    }
    i32::try_from(limit.rlim_cur).unwrap_or(i32::MAX)
}

#[cfg(target_os = "linux")]
fn last_errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Clamps a requested priority into `1..=ceiling`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn clamp_priority(priority: i32, ceiling: i32) -> i32 {
    priority.clamp(1, ceiling.max(1))
}

/// Counts and rate-limits period underruns without allocating or blocking.
///
/// Reporting every underrun is worse than useless: a dropout storm turns each
/// glitch into a locked `stderr` write on the audio thread, which causes the
/// next dropout. This accumulates instead and releases at most one summary per
/// interval. The interval is measured in periods, derived from the stream
/// format, so the audio path never needs a clock syscall to decide.
#[derive(Debug, Clone)]
pub struct XrunMonitor {
    total: u64,
    unreported: u64,
    periods_since_report: u64,
    report_interval_periods: u64,
}

/// A released underrun summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XrunReport {
    /// Underruns since the process started.
    pub total: u64,
    /// Underruns accumulated since the previous report.
    pub recent: u64,
}

impl fmt::Display for XrunReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "XRUN_RECOVERED recent={} total={}",
            self.recent, self.total
        )
    }
}

impl XrunMonitor {
    /// Builds a monitor that releases at most one summary per second of audio.
    pub fn new(sample_rate_hz: u32, period_frames: usize) -> Self {
        let periods_per_second = if period_frames == 0 {
            1
        } else {
            (sample_rate_hz as u64 / period_frames as u64).max(1)
        };
        Self {
            total: 0,
            unreported: 0,
            periods_since_report: 0,
            report_interval_periods: periods_per_second,
        }
    }

    /// Retunes the reporting interval after the stream format changes.
    ///
    /// Totals survive, because an underrun history that resets whenever the
    /// operator switches devices hides the very pattern worth noticing.
    pub fn reconfigure(&mut self, sample_rate_hz: u32, period_frames: usize) {
        let retuned = Self::new(sample_rate_hz, period_frames);
        self.report_interval_periods = retuned.report_interval_periods;
        self.periods_since_report = 0;
    }

    /// Records one recovered underrun. Allocation-free and lock-free.
    pub fn record(&mut self) {
        self.total = self.total.saturating_add(1);
        self.unreported = self.unreported.saturating_add(1);
    }

    /// Advances one period and releases a summary when one is due.
    ///
    /// The first underrun of a quiet stretch is released immediately; a storm
    /// is then collapsed into one summary per interval.
    pub fn tick(&mut self) -> Option<XrunReport> {
        self.periods_since_report = self.periods_since_report.saturating_add(1);
        if self.unreported == 0 {
            return None;
        }
        if self.total > self.unreported && self.periods_since_report < self.report_interval_periods
        {
            return None;
        }
        let report = XrunReport {
            total: self.total,
            recent: self.unreported,
        };
        self.unreported = 0;
        self.periods_since_report = 0;
        Some(report)
    }

    /// Underruns since the process started.
    pub fn total(&self) -> u64 {
        self.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_priority_is_clamped_into_the_platform_range() {
        assert_eq!(clamp_priority(75, 99), 75);
        assert_eq!(clamp_priority(120, 99), 99);
        assert_eq!(clamp_priority(0, 99), 1);
        assert_eq!(clamp_priority(-5, 99), 1);
    }

    #[test]
    fn clamping_survives_a_degenerate_platform_ceiling() {
        assert_eq!(clamp_priority(75, 0), 1);
    }

    #[test]
    fn degraded_status_names_the_remedy() {
        let status = RealtimeStatus {
            scheduling: SchedulingState::Denied {
                requested: 75,
                granted: 0,
            },
            memory: MemoryState::Denied,
        };
        assert!(status.is_degraded());
        assert!(!status.is_fully_engaged());
        assert!(status.remedy().is_some());
        assert!(status.to_string().starts_with("REALTIME_DEGRADED"));
    }

    #[test]
    fn engaged_status_reports_no_remedy() {
        let status = RealtimeStatus {
            scheduling: SchedulingState::Realtime { priority: 75 },
            memory: MemoryState::Locked,
        };
        assert!(status.is_fully_engaged());
        assert_eq!(status.remedy(), None);
        assert_eq!(
            status.to_string(),
            "REALTIME_ENGAGED scheduling=fifo:75 memory=locked"
        );
    }

    #[test]
    fn partial_engagement_is_still_degraded() {
        let status = RealtimeStatus {
            scheduling: SchedulingState::Realtime { priority: 75 },
            memory: MemoryState::Denied,
        };
        assert!(status.is_degraded());
        assert!(status.remedy().is_some());
    }

    #[test]
    fn a_quiet_stream_never_reports() {
        let mut monitor = XrunMonitor::new(48_000, 128);
        for _ in 0..1_000 {
            assert_eq!(monitor.tick(), None);
        }
        assert_eq!(monitor.total(), 0);
    }

    #[test]
    fn the_first_underrun_is_reported_immediately() {
        let mut monitor = XrunMonitor::new(48_000, 128);
        monitor.record();
        assert_eq!(
            monitor.tick(),
            Some(XrunReport {
                total: 1,
                recent: 1
            })
        );
    }

    #[test]
    fn an_underrun_storm_collapses_into_one_summary_per_interval() {
        let mut monitor = XrunMonitor::new(48_000, 128);
        // 48000 / 128 = 375 periods per report interval.
        monitor.record();
        monitor.tick();

        let mut reports = Vec::new();
        for _ in 0..375 {
            monitor.record();
            if let Some(report) = monitor.tick() {
                reports.push(report);
            }
        }

        assert_eq!(reports.len(), 1, "a storm must not report per underrun");
        assert_eq!(reports[0].recent, 375);
        assert_eq!(reports[0].total, 376);
    }

    #[test]
    fn totals_survive_across_reports() {
        let mut monitor = XrunMonitor::new(48_000, 128);
        for _ in 0..3 {
            monitor.record();
            for _ in 0..400 {
                monitor.tick();
            }
        }
        assert_eq!(monitor.total(), 3);
    }

    #[test]
    fn a_degenerate_stream_format_still_yields_a_usable_interval() {
        let mut monitor = XrunMonitor::new(0, 0);
        monitor.record();
        assert!(monitor.tick().is_some());
    }
}
