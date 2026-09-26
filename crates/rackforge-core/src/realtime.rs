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

/// Outcome of asking Windows not to throttle this process in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThrottlingState {
    /// The process is exempt: its execution speed is not reduced when it
    /// stops being the foreground window.
    Exempt,
    /// The request failed. The process stays subject to whatever the power
    /// manager decides, which on a laptop in the background means a lower
    /// clock and the efficiency cores.
    Failed { errno: i32 },
    /// Not applicable on this platform.
    Unsupported,
}

/// Outcome of asking the FPU to treat subnormal floats as zero on the
/// calling thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubnormalState {
    /// Subnormal results are flushed to zero and subnormal operands read as
    /// zero, on this thread, for as long as nothing changes the register.
    Flushed,
    /// Not applicable on this architecture.
    Unsupported,
}

/// Makes the calling thread treat subnormal floats as zero.
///
/// A recursive filter left to decay heads for zero exponentially and never
/// arrives. Below about 1.2e-38 its state turns subnormal, and x86 does
/// arithmetic on subnormals in microcode, tens of times slower than on
/// normal floats. Nothing audible is happening -- the signal is 750 dB down --
/// but the block gets several times more expensive the moment the keys
/// are released, stays that way for as long as nobody plays, and recovers
/// the instant someone does. Measured with `examples/tail-cost.rs`: RF-Organ
/// settles on an output of 2.2e-43, every sample subnormal, forever, and
/// its block costs three times what it did while playing. With the flags set,
/// the same tail costs what silence costs.
///
/// Plugins cannot be relied on to guard their own state, and the ones that
/// run inside wasmtime could not set the flags if they wanted to: the
/// register belongs to the host thread and compiled wasm inherits it. So the
/// host sets it on every thread that runs DSP. This is what every DAW does.
///
/// The flags change only results below the smallest normal float, and they
/// change them to zero.
pub fn flush_subnormals() -> SubnormalState {
    flush_subnormals_on_this_thread()
}

#[cfg(target_arch = "x86_64")]
fn flush_subnormals_on_this_thread() -> SubnormalState {
    /// MXCSR flush-to-zero (bit 15) and denormals-are-zero (bit 6).
    const FTZ_DAZ: u32 = 0x8040;
    let mut control = 0_u32;
    // SAFETY: MXCSR is per-thread state. Reading it and setting two flag
    // bits touches nothing but this thread's SSE rounding behaviour, and
    // the write is skipped when the bits are already there, so a caller on
    // every block pays one store-to-memory.
    unsafe {
        core::arch::asm!(
            "stmxcsr [{}]",
            in(reg) &raw mut control,
            options(nostack, preserves_flags),
        );
        if control & FTZ_DAZ != FTZ_DAZ {
            control |= FTZ_DAZ;
            core::arch::asm!(
                "ldmxcsr [{}]",
                in(reg) &raw const control,
                options(nostack, preserves_flags, readonly),
            );
        }
    }
    SubnormalState::Flushed
}

#[cfg(target_arch = "aarch64")]
fn flush_subnormals_on_this_thread() -> SubnormalState {
    /// FPCR.FZ: flush subnormal inputs and results to zero.
    const FZ: u64 = 1 << 24;
    let control: u64;
    // SAFETY: FPCR is per-thread state and FZ changes only how subnormal
    // floats are treated on this thread.
    unsafe {
        core::arch::asm!(
            "mrs {}, fpcr",
            out(reg) control,
            options(nomem, nostack, preserves_flags),
        );
        if control & FZ == 0 {
            core::arch::asm!(
                "msr fpcr, {}",
                in(reg) control | FZ,
                options(nomem, nostack, preserves_flags),
            );
        }
    }
    SubnormalState::Flushed
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
fn flush_subnormals_on_this_thread() -> SubnormalState {
    SubnormalState::Unsupported
}

/// Asks the platform not to slow this process down when it loses focus.
///
/// This is a *process* property, unlike [`engage`], which is a per-thread
/// one, and the two do not substitute for each other. Windows 11 applies
/// EcoQoS to a process whose windows are all in the background: the clock
/// drops and work moves to the efficiency cores. A thread promoted to MMCSS
/// "Pro Audio" is still inside that process and still runs slower, which is
/// heard as a burst of xruns the moment another window is clicked and as
/// nothing at all while RackForge is in front -- the shape of the fault that
/// prompted this.
///
/// Idempotent and safe to call from anywhere; the request is made once per
/// process. It never fails fatally, because a host that cannot obtain the
/// exemption still plays.
pub fn exempt_process_from_throttling() -> ThrottlingState {
    static ONCE: std::sync::OnceLock<ThrottlingState> = std::sync::OnceLock::new();
    ONCE.get_or_init(request_throttling_exemption).clone()
}

#[cfg(target_os = "windows")]
fn request_throttling_exemption() -> ThrottlingState {
    #[repr(C)]
    struct ProcessPowerThrottlingState {
        version: u32,
        control_mask: u32,
        state_mask: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut core::ffi::c_void;
        fn SetProcessInformation(
            process: *mut core::ffi::c_void,
            information_class: i32,
            information: *mut core::ffi::c_void,
            size: u32,
        ) -> i32;
        fn GetLastError() -> u32;
    }
    /// `ProcessPowerThrottling` in `PROCESS_INFORMATION_CLASS`.
    const PROCESS_POWER_THROTTLING: i32 = 4;
    const PROCESS_POWER_THROTTLING_CURRENT_VERSION: u32 = 1;
    const PROCESS_POWER_THROTTLING_EXECUTION_SPEED: u32 = 0x1;

    // Naming the control bit and leaving its state bit clear is how the API
    // spells "manage this explicitly, and the answer is no". Passing a zero
    // control mask would instead mean "go back to whatever the system wants",
    // which is the behaviour being turned off.
    let mut state = ProcessPowerThrottlingState {
        version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        state_mask: 0,
    };
    // SAFETY: a correctly sized, fully initialised state block for the class
    // being set, with the pseudo-handle for the current process.
    let ok = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            PROCESS_POWER_THROTTLING,
            (&raw mut state).cast(),
            u32::try_from(size_of::<ProcessPowerThrottlingState>()).unwrap_or(0),
        )
    };
    if ok != 0 {
        return ThrottlingState::Exempt;
    }
    ThrottlingState::Failed {
        // SAFETY: plain thread-local error read.
        errno: unsafe { GetLastError() } as i32,
    }
}

#[cfg(not(target_os = "windows"))]
fn request_throttling_exemption() -> ThrottlingState {
    ThrottlingState::Unsupported
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

/// Requests real-time scheduling and memory residency for the calling thread,
/// and makes it treat subnormal floats as zero.
///
/// Must be called *on the thread that runs the audio loop*: `SCHED_FIFO` is a
/// per-thread property, so engaging it from a supervisor thread protects the
/// wrong thread and produces a status that lies. The subnormal flags are
/// per-thread too, which is why they are set here: every thread that runs
/// DSP already calls this -- the audio callback, the appliance's audio loop
/// and each render pool worker -- and one that forgot would render its share
/// of a block at microcode speed. See [`flush_subnormals`].
///
/// The requested priority is clamped to the platform ceiling rather than
/// rejected, so a conservative default keeps working on kernels whose maximum
/// is lower than expected.
pub fn engage(priority: i32) -> RealtimeStatus {
    flush_subnormals();
    engage_platform(priority)
}

/// Linux: `SCHED_FIFO` and locked memory, each against its own limit.
#[cfg(target_os = "linux")]
fn engage_platform(priority: i32) -> RealtimeStatus {
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
fn engage_platform(priority: i32) -> RealtimeStatus {
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
fn engage_platform(priority: i32) -> RealtimeStatus {
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
fn engage_platform(_priority: i32) -> RealtimeStatus {
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
    fn the_throttling_exemption_is_decided_once_and_reports_the_same_answer() {
        // Asked for from every audio start and every worker; the answer has
        // to be stable, because a second call that returned something else
        // would mean the process changed posture behind the caller's back.
        let first = exempt_process_from_throttling();
        let second = exempt_process_from_throttling();
        assert_eq!(first, second);
        // A request that fails leaves the host playable, so what matters is
        // that the answer belongs to the platform: only Windows has the
        // exemption to ask for, and only Windows may report having asked.
        let asked = matches!(
            first,
            ThrottlingState::Exempt | ThrottlingState::Failed { .. }
        );
        assert_eq!(asked, cfg!(target_os = "windows"));
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn a_flushed_thread_computes_zero_where_it_would_have_produced_a_subnormal() {
        // On a thread of its own: the flags are per-thread, and a test that
        // set them on the harness's thread would change every test that ran
        // on it afterwards.
        std::thread::spawn(|| {
            let smallest_normal = std::hint::black_box(f32::MIN_POSITIVE);
            let divisor = std::hint::black_box(4.0_f32);
            assert!(
                (smallest_normal / divisor).is_subnormal(),
                "a fresh thread must start with IEEE subnormals, or this proves nothing"
            );
            assert_eq!(flush_subnormals(), SubnormalState::Flushed);
            assert_eq!(std::hint::black_box(smallest_normal) / divisor, 0.0);
            // Idempotent: asking again changes nothing and still reports it.
            assert_eq!(flush_subnormals(), SubnormalState::Flushed);
            assert_eq!(std::hint::black_box(smallest_normal) / divisor, 0.0);
        })
        .join()
        .expect("the flushed thread panicked");
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn engaging_a_thread_flushes_its_subnormals() {
        // Every DSP thread reaches the flags through `engage`, so that is
        // the path that has to set them, whatever the scheduler answered.
        std::thread::spawn(|| {
            let _ = engage(DEFAULT_AUDIO_PRIORITY);
            let smallest_normal = std::hint::black_box(f32::MIN_POSITIVE);
            assert_eq!(smallest_normal / std::hint::black_box(4.0_f32), 0.0);
        })
        .join()
        .expect("the engaged thread panicked");
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
