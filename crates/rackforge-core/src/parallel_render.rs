//! Global scheduling of ready render jobs across one pool of audio workers.
//!
//! The classic pool split Rack Slots into static per-worker ranges, which
//! made a whole plugin instance the smallest schedulable task: a five-voice
//! synthesizer occupied one worker while others sat idle. This module
//! replaces that split with a bounded job graph. Every Slot contributes
//! either one *single* job (classic plugins) or a `begin → units → end`
//! family (plugins that declare `parallel_render_v1`), and any worker that
//! finishes its current job claims the next ready job from any Slot.
//!
//! Real-time rules observed by the block-scheduling path:
//! * no allocation — every schedule structure is preallocated and bounded by
//!   [`MAX_RENDER_SLOTS`] × [`MAX_PARALLEL_UNITS`];
//! * no blocking locks — claims are single CAS transitions and completion is
//!   an atomic countdown;
//! * bounded waits — idle workers spin briefly, then park; every state
//!   transition that creates work unparks them, and the coordinator parks
//!   with a timeout as a defensive bound;
//! * no logging — faults are silenced in place and *counted*; the telemetry
//!   publisher prints from its own thread.
//!
//! WebAssembly instances are not concurrently reentrant: one `wasmtime`
//! store must never be entered from two threads at once. Parallel plugins
//! therefore get one *coordinator* instance (global state: MIDI, automation,
//! voice allocation) plus one host-owned *worker instance per unit* holding
//! that unit's persistent DSP state. Data flows between them through the
//! extension's bounded dispatch/mix buffers, copied by the host — never by
//! duplicating MIDI into cloned full instances.

use crate::{LoadedPlugin, PluginInstance};
use rackforge_plugin_api::abi::{MidiEventV1, ParameterEventV1};
use rackforge_plugin_runtime::{MAX_PARALLEL_UNITS, ParallelLayout, ParallelPlanEntry};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};
use std::{env, thread};

/// Upper bound on schedulable Slots, mirroring the control plane's
/// `MAX_ACTIVE_RACK_SLOTS`. The live host asserts the two stay equal.
pub const MAX_RENDER_SLOTS: usize = 8;

const AUDIO_WORKER_PRIORITY: i32 = crate::realtime::DEFAULT_AUDIO_PRIORITY - 1;
const WORKER_SHUTDOWN_EPOCH: u64 = u64::MAX;
/// Spin passes before an idle worker parks inside a block.
const IDLE_SPIN_PASSES: u32 = 64;
/// Defensive bound on one coordinator park; completion normally unparks it.
const COORDINATOR_PARK: Duration = Duration::from_millis(2);

const PHASE_IDLE: u8 = 0;
const PHASE_FIRST_READY: u8 = 1;
const PHASE_FIRST_RUNNING: u8 = 2;
const PHASE_UNITS: u8 = 3;
const PHASE_END_READY: u8 = 4;
const PHASE_END_RUNNING: u8 = 5;
const PHASE_DONE: u8 = 6;

/// Telemetry stage indices.
pub const STAGE_SINGLE: usize = 0;
pub const STAGE_BEGIN: usize = 1;
pub const STAGE_UNIT: usize = 2;
pub const STAGE_END: usize = 3;
pub const STAGE_COUNT: usize = 4;

pub const STAGE_NAMES: [&str; STAGE_COUNT] = ["process", "begin", "unit", "end"];

/// One ready-to-run unit job, published by the coordinator thread before the
/// block starts and consumed by exactly one worker.
///
/// The `context` pointer designates state owned exclusively by this unit for
/// the duration of the block; `run` receives it back. Keeping the shape
/// opaque lets the scheduler stay independent of the WebAssembly glue.
#[derive(Clone, Copy)]
pub struct UnitJob {
    pub context: *mut (),
    pub unit: u32,
    pub run: unsafe fn(context: *mut (), unit: u32, frames: u32, channels: u32) -> bool,
}

impl UnitJob {
    const fn empty() -> Self {
        unsafe fn never(_: *mut (), _: u32, _: u32, _: u32) -> bool {
            false
        }
        Self {
            context: std::ptr::null_mut(),
            unit: 0,
            run: never,
        }
    }
}

// SAFETY: a `UnitJob` crosses threads only under the pool's epoch protocol:
// the coordinator writes it before publishing the block and exactly one
// worker claims it. The implementor of `ScheduledSlot` promises the pointed
// state is exclusively owned by the unit.
unsafe impl Send for UnitJob {}

/// Executes one unit job. Callers must hold the unit claim for this block.
///
/// # Safety
///
/// `job` must have been produced by [`ScheduledSlot::unit_job`] for the
/// current block, and no other thread may run the same unit concurrently.
pub unsafe fn execute_unit_job(job: &UnitJob, frames: u32, channels: u32) -> bool {
    // SAFETY: forwarded from this function's contract.
    unsafe { (job.run)(job.context, job.unit, frames, channels) }
}

/// One schedulable Slot.
///
/// # Safety
///
/// Implementors promise that:
/// * every method may be called from any pool-owned thread (instances are
///   never destroyed by the pool, only entered);
/// * [`Self::unit_job`] returns jobs whose state is disjoint per unit, so
///   different units of one Slot may run concurrently;
/// * `run_begin` only reports units below `max_units`.
pub unsafe trait ScheduledSlot {
    /// Number of host-schedulable units; `0` renders as one classic job.
    fn max_units(&self) -> u32;
    /// Classic whole-plugin render. Returns `false` on a fault.
    fn run_single(&mut self, frames: u32, channels: u32) -> bool;
    /// Serial pre-stage. Returns the bitmask of units to schedule, or `None`
    /// on a fault. A mask of `0` skips straight to `run_end`.
    fn run_begin(&mut self, frames: u32, channels: u32) -> Option<u32>;
    /// Publishes the pointer table for one unit. Called on the coordinator
    /// thread before the block is released to the workers.
    fn unit_job(&mut self, unit: u32, frames: u32, channels: u32) -> UnitJob;
    /// Serial post-stage. `completed` carries the units that rendered
    /// successfully; the Slot must silence the rest deterministically.
    fn run_end(&mut self, frames: u32, channels: u32, completed: u32) -> bool;
    /// Silences the Slot after a fault until the host rebuilds it.
    fn quarantine(&mut self);
}

// ---------------------------------------------------------------------------
// Telemetry
// ---------------------------------------------------------------------------

/// Log-scale histogram: index = (ilog2(ns) << 4) | next-4-mantissa-bits,
/// which resolves durations to about six percent — plenty for p95/p99.
const HISTOGRAM_BUCKETS: usize = 1024;

pub struct Histogram {
    count: AtomicU64,
    sum_ns: AtomicU64,
    max_ns: AtomicU64,
    buckets: Box<[AtomicU32]>,
}

impl Histogram {
    fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            sum_ns: AtomicU64::new(0),
            max_ns: AtomicU64::new(0),
            buckets: (0..HISTOGRAM_BUCKETS).map(|_| AtomicU32::new(0)).collect(),
        }
    }

    fn bucket_index(ns: u64) -> usize {
        let ns = ns.max(1);
        let exponent = 63 - ns.leading_zeros() as u64;
        let fraction = if exponent >= 4 {
            (ns >> (exponent - 4)) & 0xF
        } else {
            (ns << (4 - exponent)) & 0xF
        };
        ((exponent << 4) | fraction) as usize
    }

    fn bucket_lower_bound(index: usize) -> u64 {
        let exponent = (index >> 4) as u64;
        let fraction = (index & 0xF) as u64;
        if exponent >= 4 {
            (1 << exponent) | (fraction << (exponent - 4))
        } else {
            1 << exponent
        }
    }

    fn record(&self, ns: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_ns.fetch_add(ns, Ordering::Relaxed);
        self.max_ns.fetch_max(ns, Ordering::Relaxed);
        self.buckets[Self::bucket_index(ns)].fetch_add(1, Ordering::Relaxed);
    }

    fn drain(&self) -> HistogramSnapshot {
        let count = self.count.swap(0, Ordering::Relaxed);
        let sum_ns = self.sum_ns.swap(0, Ordering::Relaxed);
        let max_ns = self.max_ns.swap(0, Ordering::Relaxed);
        let mut buckets = [0_u32; HISTOGRAM_BUCKETS];
        for (value, bucket) in buckets.iter_mut().zip(self.buckets.iter()) {
            *value = bucket.swap(0, Ordering::Relaxed);
        }
        HistogramSnapshot {
            count,
            sum_ns,
            max_ns,
            buckets,
        }
    }
}

#[derive(Clone)]
pub struct HistogramSnapshot {
    pub count: u64,
    pub sum_ns: u64,
    pub max_ns: u64,
    buckets: [u32; HISTOGRAM_BUCKETS],
}

impl HistogramSnapshot {
    pub fn average_ns(&self) -> u64 {
        self.sum_ns.checked_div(self.count).unwrap_or(0)
    }

    /// Lower bound of the bucket containing the requested quantile.
    pub fn percentile_ns(&self, percentile: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let rank = ((self.count as f64) * percentile / 100.0).ceil().max(1.0) as u64;
        let mut seen = 0_u64;
        for (index, bucket) in self.buckets.iter().enumerate() {
            seen += u64::from(*bucket);
            if seen >= rank {
                return Histogram::bucket_lower_bound(index);
            }
        }
        self.max_ns
    }
}

/// Real-time-safe counters written by render jobs and drained by the
/// publisher thread. Everything is preallocated at construction.
pub struct RenderTelemetry {
    stages: Box<[[Histogram; STAGE_COUNT]]>,
    block: Histogram,
    deadline_ns: AtomicU64,
    deadline_misses: AtomicU64,
    miss_attribution: Box<[[AtomicU64; STAGE_COUNT]]>,
    slot_faults: Box<[AtomicU64]>,
    unit_faults: Box<[AtomicU64]>,
    worker_units: Box<[AtomicU64]>,
    worker_busy_ns: Box<[AtomicU64]>,
    /// Publisher-side only; never touched by render threads.
    labels: Mutex<Vec<String>>,
}

impl RenderTelemetry {
    pub fn new(worker_capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            stages: (0..MAX_RENDER_SLOTS)
                .map(|_| std::array::from_fn(|_| Histogram::new()))
                .collect(),
            block: Histogram::new(),
            deadline_ns: AtomicU64::new(0),
            deadline_misses: AtomicU64::new(0),
            miss_attribution: (0..MAX_RENDER_SLOTS)
                .map(|_| std::array::from_fn(|_| AtomicU64::new(0)))
                .collect(),
            slot_faults: (0..MAX_RENDER_SLOTS).map(|_| AtomicU64::new(0)).collect(),
            unit_faults: (0..MAX_RENDER_SLOTS).map(|_| AtomicU64::new(0)).collect(),
            worker_units: (0..worker_capacity.max(1))
                .map(|_| AtomicU64::new(0))
                .collect(),
            worker_busy_ns: (0..worker_capacity.max(1))
                .map(|_| AtomicU64::new(0))
                .collect(),
            labels: Mutex::new(Vec::new()),
        })
    }

    fn record_stage(&self, slot: usize, stage: usize, ns: u64) {
        if let Some(stages) = self.stages.get(slot) {
            stages[stage].record(ns);
        }
    }

    fn record_slot_fault(&self, slot: usize) {
        if let Some(counter) = self.slot_faults.get(slot) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_unit_fault(&self, slot: usize) {
        if let Some(counter) = self.unit_faults.get(slot) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_worker(&self, worker: usize, units: u64, busy_ns: u64) {
        if let Some(counter) = self.worker_units.get(worker) {
            counter.fetch_add(units, Ordering::Relaxed);
        }
        if let Some(counter) = self.worker_busy_ns.get(worker) {
            counter.fetch_add(busy_ns, Ordering::Relaxed);
        }
    }

    /// Records one completed block: total render duration against the block
    /// deadline, plus the Slot/stage that dominated a missed deadline.
    pub fn record_block(&self, render_ns: u64, deadline_ns: u64, culprit: Option<(usize, usize)>) {
        self.block.record(render_ns);
        self.deadline_ns.store(deadline_ns, Ordering::Relaxed);
        if render_ns > deadline_ns {
            self.deadline_misses.fetch_add(1, Ordering::Relaxed);
            if let Some((slot, stage)) = culprit
                && let Some(slots) = self.miss_attribution.get(slot)
            {
                slots[stage].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Publisher-side naming of Slot indices. Called from control paths.
    pub fn set_slot_labels(&self, labels: Vec<String>) {
        if let Ok(mut guard) = self.labels.lock() {
            *guard = labels;
        }
    }

    /// Drains every counter into an owned snapshot. Publisher-side.
    pub fn snapshot_and_reset(&self) -> TelemetrySnapshot {
        let labels = self
            .labels
            .lock()
            .map(|labels| labels.clone())
            .unwrap_or_default();
        TelemetrySnapshot {
            stages: self
                .stages
                .iter()
                .map(|stages| std::array::from_fn(|stage| stages[stage].drain()))
                .collect(),
            block: self.block.drain(),
            deadline_ns: self.deadline_ns.load(Ordering::Relaxed),
            deadline_misses: self.deadline_misses.swap(0, Ordering::Relaxed),
            miss_attribution: self
                .miss_attribution
                .iter()
                .map(|slots| std::array::from_fn(|stage| slots[stage].swap(0, Ordering::Relaxed)))
                .collect(),
            slot_faults: self
                .slot_faults
                .iter()
                .map(|counter| counter.swap(0, Ordering::Relaxed))
                .collect(),
            unit_faults: self
                .unit_faults
                .iter()
                .map(|counter| counter.swap(0, Ordering::Relaxed))
                .collect(),
            worker_units: self
                .worker_units
                .iter()
                .map(|counter| counter.swap(0, Ordering::Relaxed))
                .collect(),
            worker_busy_ns: self
                .worker_busy_ns
                .iter()
                .map(|counter| counter.swap(0, Ordering::Relaxed))
                .collect(),
            labels,
        }
    }
}

pub struct TelemetrySnapshot {
    pub stages: Vec<[HistogramSnapshot; STAGE_COUNT]>,
    pub block: HistogramSnapshot,
    pub deadline_ns: u64,
    pub deadline_misses: u64,
    pub miss_attribution: Vec<[u64; STAGE_COUNT]>,
    pub slot_faults: Vec<u64>,
    pub unit_faults: Vec<u64>,
    pub worker_units: Vec<u64>,
    pub worker_busy_ns: Vec<u64>,
    pub labels: Vec<String>,
}

impl TelemetrySnapshot {
    fn label(&self, slot: usize) -> String {
        self.labels
            .get(slot)
            .cloned()
            .unwrap_or_else(|| format!("slot-{slot}"))
    }

    /// Renders the snapshot as the host's key=value log lines.
    pub fn render_lines(&self, elapsed: Duration) -> Vec<String> {
        let mut lines = Vec::new();
        if self.block.count > 0 {
            let deadline = self.deadline_ns.max(1);
            let average_pct = self.block.average_ns() as f64 * 100.0 / deadline as f64;
            let max_pct = self.block.max_ns as f64 * 100.0 / deadline as f64;
            lines.push(format!(
                "AUDIO_RENDER_BLOCK blocks={} avg_us={} p95_us={} p99_us={} max_us={} \
                 deadline_us={} budget_avg_pct={average_pct:.1} budget_max_pct={max_pct:.1} \
                 deadline_misses={}",
                self.block.count,
                self.block.average_ns() / 1_000,
                self.block.percentile_ns(95.0) / 1_000,
                self.block.percentile_ns(99.0) / 1_000,
                self.block.max_ns / 1_000,
                self.deadline_ns / 1_000,
                self.deadline_misses,
            ));
        }
        for (slot, stages) in self.stages.iter().enumerate() {
            for (stage, histogram) in stages.iter().enumerate() {
                if histogram.count == 0 {
                    continue;
                }
                lines.push(format!(
                    "AUDIO_RENDER_STAGE slot={} stage={} count={} avg_us={} p95_us={} \
                     p99_us={} max_us={}",
                    self.label(slot),
                    STAGE_NAMES[stage],
                    histogram.count,
                    histogram.average_ns() / 1_000,
                    histogram.percentile_ns(95.0) / 1_000,
                    histogram.percentile_ns(99.0) / 1_000,
                    histogram.max_ns / 1_000,
                ));
            }
        }
        for (slot, stages) in self.miss_attribution.iter().enumerate() {
            for (stage, count) in stages.iter().enumerate() {
                if *count > 0 {
                    lines.push(format!(
                        "AUDIO_RENDER_DEADLINE_MISS slot={} stage={} count={count}",
                        self.label(slot),
                        STAGE_NAMES[stage],
                    ));
                }
            }
        }
        for (slot, count) in self.slot_faults.iter().enumerate() {
            if *count > 0 {
                lines.push(format!(
                    "AUDIO_RENDER_SLOT_FAULT slot={} count={count} action=quarantine",
                    self.label(slot)
                ));
            }
        }
        for (slot, count) in self.unit_faults.iter().enumerate() {
            if *count > 0 {
                lines.push(format!(
                    "AUDIO_RENDER_UNIT_FAULT slot={} count={count} action=unit-silenced",
                    self.label(slot)
                ));
            }
        }
        let busy: Vec<String> = self
            .worker_busy_ns
            .iter()
            .map(|ns| {
                let pct = *ns as f64 * 100.0 / elapsed.as_nanos().max(1) as f64;
                format!("{pct:.1}")
            })
            .collect();
        let units: Vec<String> = self
            .worker_units
            .iter()
            .map(|count| count.to_string())
            .collect();
        if self.worker_units.iter().any(|count| *count > 0) {
            lines.push(format!(
                "AUDIO_RENDER_WORKERS units=[{}] busy_pct=[{}]",
                units.join(","),
                busy.join(","),
            ));
        }
        lines
    }
}

/// Prints telemetry from its own thread; the audio path only bumps atomics.
/// The publisher exits by itself once the pool (and its `Arc`) is gone.
pub fn spawn_telemetry_publisher(telemetry: &Arc<RenderTelemetry>, interval: Duration) {
    let weak: Weak<RenderTelemetry> = Arc::downgrade(telemetry);
    let _ = thread::Builder::new()
        .name("rackforge-render-telemetry".into())
        .spawn(move || {
            let mut last = Instant::now();
            loop {
                thread::sleep(interval);
                let Some(telemetry) = weak.upgrade() else {
                    return;
                };
                let elapsed = last.elapsed();
                last = Instant::now();
                for line in telemetry.snapshot_and_reset().render_lines(elapsed) {
                    println!("{line}");
                }
            }
        });
}

// ---------------------------------------------------------------------------
// Worker pool
// ---------------------------------------------------------------------------

struct SlotSchedule {
    phase: AtomicU8,
    pending_units: AtomicU32,
    remaining_units: AtomicU32,
    completed_units: AtomicU32,
    block_ns: AtomicU64,
    stage_ns: [AtomicU64; STAGE_COUNT],
}

impl SlotSchedule {
    fn new() -> Self {
        Self {
            phase: AtomicU8::new(PHASE_IDLE),
            pending_units: AtomicU32::new(0),
            remaining_units: AtomicU32::new(0),
            completed_units: AtomicU32::new(0),
            block_ns: AtomicU64::new(0),
            stage_ns: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

struct PoolShared<S> {
    epoch: AtomicU64,
    remaining_jobs: AtomicUsize,
    slot_count: AtomicUsize,
    frames: AtomicU32,
    channels: AtomicU32,
    slots_ptr: UnsafeCell<*mut S>,
    coordinator: UnsafeCell<Option<thread::Thread>>,
    schedules: Vec<SlotSchedule>,
    unit_jobs: Vec<[UnsafeCell<UnitJob>; MAX_PARALLEL_UNITS]>,
    worker_threads: OnceLock<Box<[thread::Thread]>>,
    completed_epochs: Vec<AtomicU64>,
    telemetry: Arc<RenderTelemetry>,
}

// SAFETY: all cross-thread access is mediated by the epoch protocol — the
// coordinator writes the UnsafeCell contents before an Release epoch store
// and workers read them after an Acquire load; Slot state is only entered
// under a CAS claim; `ScheduledSlot` is an unsafe trait whose implementors
// promise thread-migration safety.
unsafe impl<S> Send for PoolShared<S> {}
// SAFETY: as above.
unsafe impl<S> Sync for PoolShared<S> {}

impl<S> PoolShared<S> {
    fn wake_workers(&self, active: usize) {
        if let Some(threads) = self.worker_threads.get() {
            for thread in threads.iter().take(active) {
                thread.unpark();
            }
        }
    }

    fn wake_coordinator(&self) {
        // SAFETY: written by the coordinator before the epoch publication
        // that allowed this worker to run; only cloned handles escape.
        if let Some(coordinator) = unsafe { (*self.coordinator.get()).as_ref() } {
            coordinator.unpark();
        }
    }
}

pub fn automatic_audio_worker_capacity(available_cpus: usize) -> usize {
    match available_cpus {
        0 | 1 => 0,
        // On two-core systems the coordinator sleeps while both workers run.
        // On larger systems one core remains available for the coordinator,
        // device IRQs and the rest of the host.
        2 => 2,
        count => count - 1,
    }
    .min(MAX_RENDER_SLOTS)
}

pub fn requested_audio_worker_capacity(available_cpus: usize) -> usize {
    let automatic = automatic_audio_worker_capacity(available_cpus);
    let Some(value) = env::var_os("RACKFORGE_AUDIO_WORKERS") else {
        return automatic;
    };
    let Some(value) = value.to_str() else {
        eprintln!("AUDIO_WORKERS_INVALID value=non-utf8 fallback=auto:{automatic}");
        return automatic;
    };
    match value.parse::<usize>() {
        Ok(requested) => requested.min(MAX_RENDER_SLOTS),
        Err(_) => {
            eprintln!("AUDIO_WORKERS_INVALID value={value:?} fallback=auto:{automatic}");
            automatic
        }
    }
}

/// Whether this host will schedule units at all. With fewer than two
/// workers the sequential `rackforge_process` fallback of the very same
/// package is used instead, and no unit instances are created — creating
/// them would move per-unit DSP state into instances nothing ever runs.
/// The decision is stable for the whole process so a Rack never switches
/// state location between blocks.
pub fn parallel_units_enabled() -> bool {
    let available_cpus = thread::available_parallelism().map_or(1, |count| count.get());
    requested_audio_worker_capacity(available_cpus) >= 2
}

pub struct RenderPool<S: ScheduledSlot + 'static> {
    shared: Arc<PoolShared<S>>,
    handles: Vec<thread::JoinHandle<()>>,
    epoch: u64,
}

impl<S: ScheduledSlot + 'static> RenderPool<S> {
    /// Creates the pool with the automatic worker count for this machine,
    /// honoring `RACKFORGE_AUDIO_WORKERS` exactly as the previous pool did.
    pub fn automatic(telemetry: Arc<RenderTelemetry>) -> Self {
        let available_cpus = thread::available_parallelism().map_or(1, |count| count.get());
        let requested = requested_audio_worker_capacity(available_cpus);
        let pool = Self::with_workers(requested, telemetry);
        println!(
            "AUDIO_PARALLEL_READY mode={} detected_cpus={available_cpus} workers={}",
            if env::var_os("RACKFORGE_AUDIO_WORKERS").is_some() {
                "manual"
            } else {
                "auto"
            },
            pool.worker_count()
        );
        pool
    }

    pub fn with_workers(requested: usize, telemetry: Arc<RenderTelemetry>) -> Self {
        let shared = Arc::new(PoolShared {
            epoch: AtomicU64::new(0),
            remaining_jobs: AtomicUsize::new(0),
            slot_count: AtomicUsize::new(0),
            frames: AtomicU32::new(0),
            channels: AtomicU32::new(0),
            slots_ptr: UnsafeCell::new(std::ptr::null_mut()),
            coordinator: UnsafeCell::new(None),
            schedules: (0..MAX_RENDER_SLOTS).map(|_| SlotSchedule::new()).collect(),
            unit_jobs: (0..MAX_RENDER_SLOTS)
                .map(|_| std::array::from_fn(|_| UnsafeCell::new(UnitJob::empty())))
                .collect(),
            worker_threads: OnceLock::new(),
            completed_epochs: (0..requested).map(|_| AtomicU64::new(0)).collect(),
            telemetry,
        });
        let mut handles = Vec::with_capacity(requested);
        for index in 0..requested {
            let worker_shared = Arc::clone(&shared);
            match thread::Builder::new()
                .name(format!("rackforge-audio-worker-{index}"))
                .spawn(move || worker_main(index, worker_shared))
            {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    eprintln!(
                        "AUDIO_WORKER_SPAWN_FAILED index={index} error={error} active_workers={}",
                        handles.len()
                    );
                    break;
                }
            }
        }
        let threads: Box<[thread::Thread]> = handles
            .iter()
            .map(|handle| handle.thread().clone())
            .collect();
        let _ = shared.worker_threads.set(threads);
        Self {
            shared,
            handles,
            epoch: 0,
        }
    }

    pub fn worker_count(&self) -> usize {
        self.handles.len()
    }

    pub fn telemetry(&self) -> &Arc<RenderTelemetry> {
        &self.shared.telemetry
    }

    /// Schedules one block across the pool. Returns `false` when serial
    /// execution is cheaper or is the only safe option — the caller then
    /// runs [`process_slots_sequential`] over the very same graph.
    pub fn process(
        &mut self,
        slots: &mut [S],
        frames: u32,
        channels: u32,
        deadline_ns: u64,
    ) -> bool {
        let workers = self.handles.len();
        if workers < 2 || slots.is_empty() || slots.len() > MAX_RENDER_SLOTS {
            return false;
        }
        if slots.len() == 1 && slots[0].max_units() == 0 {
            return false;
        }
        let started = Instant::now();
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 || self.epoch == WORKER_SHUTDOWN_EPOCH {
            self.epoch = 1;
        }
        let epoch = self.epoch;
        let shared = &self.shared;

        // Publish the block: every write below happens-before the Release
        // store of the epoch that lets workers observe it.
        for (index, slot) in slots.iter_mut().enumerate() {
            let schedule = &shared.schedules[index];
            schedule.pending_units.store(0, Ordering::Relaxed);
            schedule.remaining_units.store(0, Ordering::Relaxed);
            schedule.completed_units.store(0, Ordering::Relaxed);
            schedule.block_ns.store(0, Ordering::Relaxed);
            for stage in &schedule.stage_ns {
                stage.store(0, Ordering::Relaxed);
            }
            let max_units = slot.max_units().min(MAX_PARALLEL_UNITS as u32);
            for unit in 0..max_units {
                // SAFETY: no worker observes this epoch yet.
                unsafe {
                    *shared.unit_jobs[index][unit as usize].get() =
                        slot.unit_job(unit, frames, channels);
                }
            }
            schedule.phase.store(PHASE_FIRST_READY, Ordering::Relaxed);
        }
        shared.slot_count.store(slots.len(), Ordering::Relaxed);
        shared.frames.store(frames, Ordering::Relaxed);
        shared.channels.store(channels, Ordering::Relaxed);
        shared.remaining_jobs.store(slots.len(), Ordering::Relaxed);
        // SAFETY: previous epoch fully acknowledged, so no reader is live.
        unsafe {
            *shared.slots_ptr.get() = slots.as_mut_ptr();
            *shared.coordinator.get() = Some(thread::current());
        }
        shared.epoch.store(epoch, Ordering::Release);
        shared.wake_workers(workers);

        // The coordinator sleeps: its core stays available for device IRQs
        // and the rest of the host. Parking is bounded defensively; workers
        // unpark it on completion and on their epoch acknowledgement.
        loop {
            let done = shared.remaining_jobs.load(Ordering::Acquire) == 0
                && shared
                    .completed_epochs
                    .iter()
                    .take(workers)
                    .all(|completed| completed.load(Ordering::Acquire) == epoch);
            if done {
                break;
            }
            thread::park_timeout(COORDINATOR_PARK);
        }

        let render_ns = started.elapsed().as_nanos() as u64;
        let culprit = (render_ns > deadline_ns)
            .then(|| {
                let mut worst = None;
                for (index, schedule) in shared.schedules.iter().take(slots.len()).enumerate() {
                    let slot_ns = schedule.block_ns.load(Ordering::Relaxed);
                    if worst.is_none_or(|(_, _, ns)| slot_ns > ns) {
                        let stage = schedule
                            .stage_ns
                            .iter()
                            .enumerate()
                            .max_by_key(|(_, ns)| ns.load(Ordering::Relaxed))
                            .map_or(STAGE_SINGLE, |(stage, _)| stage);
                        worst = Some((index, stage, slot_ns));
                    }
                }
                worst.map(|(slot, stage, _)| (slot, stage))
            })
            .flatten();
        shared
            .telemetry
            .record_block(render_ns, deadline_ns, culprit);
        true
    }
}

impl<S: ScheduledSlot + 'static> Drop for RenderPool<S> {
    fn drop(&mut self) {
        self.shared
            .epoch
            .store(WORKER_SHUTDOWN_EPOCH, Ordering::Release);
        self.shared.wake_workers(self.handles.len());
        for handle in self.handles.drain(..) {
            let _ = handle.join();
        }
    }
}

fn worker_main<S: ScheduledSlot>(index: usize, shared: Arc<PoolShared<S>>) {
    let realtime_status = crate::realtime::engage(AUDIO_WORKER_PRIORITY);
    println!("AUDIO_WORKER_READY index={index} {realtime_status}");
    let mut observed = 0_u64;
    loop {
        let epoch = shared.epoch.load(Ordering::Acquire);
        if epoch == observed {
            thread::park();
            continue;
        }
        if epoch == WORKER_SHUTDOWN_EPOCH {
            return;
        }
        run_epoch(index, &shared);
        shared.completed_epochs[index].store(epoch, Ordering::Release);
        shared.wake_coordinator();
        observed = epoch;
    }
}

fn run_epoch<S: ScheduledSlot>(worker_index: usize, shared: &PoolShared<S>) {
    let slot_count = shared.slot_count.load(Ordering::Relaxed);
    let frames = shared.frames.load(Ordering::Relaxed);
    let channels = shared.channels.load(Ordering::Relaxed);
    // SAFETY: published before the epoch this worker acquired.
    let slots_ptr = unsafe { *shared.slots_ptr.get() };
    let mut idle_passes = 0_u32;
    loop {
        if shared.remaining_jobs.load(Ordering::Acquire) == 0 {
            return;
        }
        let mut ran_any = false;
        for offset in 0..slot_count {
            let index = (worker_index + offset) % slot_count;
            if try_slot_job(shared, slots_ptr, index, frames, channels, worker_index) {
                ran_any = true;
            }
        }
        if ran_any {
            idle_passes = 0;
        } else {
            idle_passes += 1;
            if idle_passes < IDLE_SPIN_PASSES {
                std::hint::spin_loop();
            } else {
                if shared.remaining_jobs.load(Ordering::Acquire) == 0 {
                    return;
                }
                // Producers unpark every worker after publishing new jobs
                // and after the final job completes, so this park is woken
                // by the transitions it waits for.
                thread::park();
                idle_passes = 0;
            }
        }
    }
}

fn finish_job<S>(shared: &PoolShared<S>) {
    if shared.remaining_jobs.fetch_sub(1, Ordering::AcqRel) == 1 {
        let workers = shared
            .worker_threads
            .get()
            .map_or(0, |threads| threads.len());
        shared.wake_workers(workers);
        shared.wake_coordinator();
    }
}

fn try_slot_job<S: ScheduledSlot>(
    shared: &PoolShared<S>,
    slots_ptr: *mut S,
    index: usize,
    frames: u32,
    channels: u32,
    worker_index: usize,
) -> bool {
    let schedule = &shared.schedules[index];
    match schedule.phase.load(Ordering::Acquire) {
        PHASE_FIRST_READY => {
            if schedule
                .phase
                .compare_exchange(
                    PHASE_FIRST_READY,
                    PHASE_FIRST_RUNNING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                return false;
            }
            // SAFETY: the CAS above grants exclusive Slot access.
            let slot = unsafe { &mut *slots_ptr.add(index) };
            let started = Instant::now();
            if slot.max_units() == 0 {
                let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    slot.run_single(frames, channels)
                }))
                .unwrap_or(false);
                let ns = started.elapsed().as_nanos() as u64;
                record_stage(shared, schedule, index, STAGE_SINGLE, ns, worker_index, 0);
                if !ok {
                    quarantine_slot(shared, slot, index);
                }
                schedule.phase.store(PHASE_DONE, Ordering::Release);
                finish_job(shared);
            } else {
                let mask = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    slot.run_begin(frames, channels)
                }))
                .unwrap_or(None);
                let ns = started.elapsed().as_nanos() as u64;
                record_stage(shared, schedule, index, STAGE_BEGIN, ns, worker_index, 0);
                match mask {
                    Some(mask) => {
                        let count = mask.count_ones();
                        if count > 0 {
                            // Ordering matters: the new jobs are counted
                            // before this begin job retires, so the block
                            // can never be observed complete too early.
                            shared
                                .remaining_jobs
                                .fetch_add(count as usize + 1, Ordering::AcqRel);
                            schedule.remaining_units.store(count, Ordering::Relaxed);
                            schedule.pending_units.store(mask, Ordering::Release);
                            schedule.phase.store(PHASE_UNITS, Ordering::Release);
                        } else {
                            shared.remaining_jobs.fetch_add(1, Ordering::AcqRel);
                            schedule.phase.store(PHASE_END_READY, Ordering::Release);
                        }
                        let workers = shared
                            .worker_threads
                            .get()
                            .map_or(0, |threads| threads.len());
                        shared.wake_workers(workers);
                        finish_job(shared);
                    }
                    None => {
                        quarantine_slot(shared, slot, index);
                        schedule.phase.store(PHASE_DONE, Ordering::Release);
                        finish_job(shared);
                    }
                }
            }
            true
        }
        PHASE_UNITS => {
            loop {
                let pending = schedule.pending_units.load(Ordering::Acquire);
                if pending == 0 {
                    return false;
                }
                let bit = pending.isolate_lowest_one();
                if schedule
                    .pending_units
                    .compare_exchange(pending, pending & !bit, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    continue;
                }
                let unit = bit.trailing_zeros();
                // SAFETY: published at block setup; this worker holds the
                // exclusive claim on this unit for this block.
                let job = unsafe { *shared.unit_jobs[index][unit as usize].get() };
                let started = Instant::now();
                let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // SAFETY: produced by `unit_job` for this block; claimed once.
                    unsafe { execute_unit_job(&job, frames, channels) }
                }))
                .unwrap_or(false);
                let ns = started.elapsed().as_nanos() as u64;
                record_stage(shared, schedule, index, STAGE_UNIT, ns, worker_index, 1);
                if ok {
                    schedule.completed_units.fetch_or(bit, Ordering::AcqRel);
                } else {
                    shared.telemetry.record_unit_fault(index);
                }
                if schedule.remaining_units.fetch_sub(1, Ordering::AcqRel) == 1 {
                    schedule.phase.store(PHASE_END_READY, Ordering::Release);
                    let workers = shared
                        .worker_threads
                        .get()
                        .map_or(0, |threads| threads.len());
                    shared.wake_workers(workers);
                }
                finish_job(shared);
                return true;
            }
        }
        PHASE_END_READY => {
            if schedule
                .phase
                .compare_exchange(
                    PHASE_END_READY,
                    PHASE_END_RUNNING,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_err()
            {
                return false;
            }
            // SAFETY: begin retired and every unit completed, so the CAS
            // grants exclusive Slot access again.
            let slot = unsafe { &mut *slots_ptr.add(index) };
            let completed = schedule.completed_units.load(Ordering::Acquire);
            let started = Instant::now();
            let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                slot.run_end(frames, channels, completed)
            }))
            .unwrap_or(false);
            let ns = started.elapsed().as_nanos() as u64;
            record_stage(shared, schedule, index, STAGE_END, ns, worker_index, 0);
            if !ok {
                quarantine_slot(shared, slot, index);
            }
            schedule.phase.store(PHASE_DONE, Ordering::Release);
            finish_job(shared);
            true
        }
        _ => false,
    }
}

fn record_stage<S>(
    shared: &PoolShared<S>,
    schedule: &SlotSchedule,
    slot: usize,
    stage: usize,
    ns: u64,
    worker: usize,
    units: u64,
) {
    shared.telemetry.record_stage(slot, stage, ns);
    shared.telemetry.record_worker(worker, units, ns);
    schedule.block_ns.fetch_add(ns, Ordering::Relaxed);
    schedule.stage_ns[stage].fetch_add(ns, Ordering::Relaxed);
}

fn quarantine_slot<S: ScheduledSlot>(shared: &PoolShared<S>, slot: &mut S, index: usize) {
    shared.telemetry.record_slot_fault(index);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| slot.quarantine()));
}

/// Executes the same job graph in Slot order on the calling thread. This is
/// the deterministic fallback for single-core machines, `graph_audio` racks
/// and every platform without host threads. It performs the identical
/// begin → units (ascending) → end sequence on the identical instances, so
/// its output matches the pool bit for bit.
pub fn process_slots_sequential<S: ScheduledSlot>(
    slots: &mut [S],
    frames: u32,
    channels: u32,
    telemetry: &RenderTelemetry,
) {
    for (index, slot) in slots.iter_mut().enumerate() {
        process_one_slot_sequential(slot, index, frames, channels, telemetry);
    }
}

/// Sequentially renders one Slot through its scheduled stages.
pub fn process_one_slot_sequential<S: ScheduledSlot>(
    slot: &mut S,
    index: usize,
    frames: u32,
    channels: u32,
    telemetry: &RenderTelemetry,
) {
    if slot.max_units() == 0 {
        let started = Instant::now();
        let ok = slot.run_single(frames, channels);
        telemetry.record_stage(index, STAGE_SINGLE, started.elapsed().as_nanos() as u64);
        if !ok {
            telemetry.record_slot_fault(index);
            slot.quarantine();
        }
        return;
    }
    let started = Instant::now();
    let Some(mask) = slot.run_begin(frames, channels) else {
        telemetry.record_stage(index, STAGE_BEGIN, started.elapsed().as_nanos() as u64);
        telemetry.record_slot_fault(index);
        slot.quarantine();
        return;
    };
    telemetry.record_stage(index, STAGE_BEGIN, started.elapsed().as_nanos() as u64);
    let mut completed = 0_u32;
    let mut pending = mask;
    while pending != 0 {
        let bit = pending.isolate_lowest_one();
        pending &= !bit;
        let unit = bit.trailing_zeros();
        let job = slot.unit_job(unit, frames, channels);
        let started = Instant::now();
        // SAFETY: the job was just produced for this block and this thread
        // holds the whole Slot exclusively.
        let ok = unsafe { execute_unit_job(&job, frames, channels) };
        telemetry.record_stage(index, STAGE_UNIT, started.elapsed().as_nanos() as u64);
        if ok {
            completed |= bit;
        } else {
            telemetry.record_unit_fault(index);
        }
    }
    let started = Instant::now();
    let ok = slot.run_end(frames, channels, completed);
    telemetry.record_stage(index, STAGE_END, started.elapsed().as_nanos() as u64);
    if !ok {
        telemetry.record_slot_fault(index);
        slot.quarantine();
    }
}

// ---------------------------------------------------------------------------
// Host-owned unit instances for `parallel_render_v1` plugins
// ---------------------------------------------------------------------------

/// Per-unit worker state: one isolated WebAssembly instance plus the staged
/// dispatch payload and the unit's output for the current block. Each cell
/// is boxed so its address stays stable while workers hold raw pointers.
struct UnitCell<'plugin> {
    unit: u32,
    instance: PluginInstance<'plugin>,
    payload: Box<[u8]>,
    payload_len: usize,
    output: Box<[f32]>,
    input_ptr: *const f32,
    input_len: usize,
    output_samples: usize,
}

/// Runs one unit inside its worker instance.
///
/// # Safety
///
/// `context` must point at the `UnitCell` published for this unit and block,
/// with no other thread touching that cell until the block retires.
unsafe fn run_unit_cell(context: *mut (), _unit: u32, _frames: u32, _channels: u32) -> bool {
    // SAFETY: forwarded from this function's contract. The lifetime is
    // erased on the way through the scheduler; the pool never outlives the
    // Slot that owns this cell.
    let cell = unsafe { &mut *(context as *mut UnitCell<'static>) };
    let frames = _frames;
    let payload_len = cell.payload_len;
    let input = if cell.input_len == 0 {
        &[][..]
    } else {
        // SAFETY: the input buffer is owned by the Slot and only read here.
        unsafe { std::slice::from_raw_parts(cell.input_ptr, cell.input_len) }
    };
    if cell.output.len() < cell.output_samples {
        return false;
    }
    let unit = cell.unit;
    let payload_ok = cell
        .instance
        .parallel_write_dispatch(unit, &cell.payload[..payload_len])
        .is_ok();
    if !payload_ok {
        return false;
    }
    let output_samples = cell.output_samples;
    cell.instance
        .parallel_render_unit(
            unit,
            payload_len,
            input,
            &mut cell.output[..output_samples],
            frames,
        )
        .is_ok()
}

/// Host-owned worker instances and buffers for one `parallel_render_v1`
/// Slot. The coordinator instance stays with the Slot itself; this state
/// owns everything the units need.
// Boxed cells on purpose: workers hold raw pointers into them, so each
// cell's address must be independent of the containing vector.
#[allow(clippy::vec_box)]
pub struct ParallelUnits<'plugin> {
    layout: ParallelLayout,
    cells: Vec<Box<UnitCell<'plugin>>>,
    plan: Box<[ParallelPlanEntry]>,
    plan_mask: u32,
    sched_mask: u32,
    quarantined_units: u32,
    maximum_frames: u32,
    input_channels: u32,
    output_channels: u32,
}

impl<'plugin> ParallelUnits<'plugin> {
    /// Creates one worker instance per unit and prepares them all. Returns
    /// `None` when the plugin does not expose the extension. This runs on a
    /// control/setup thread — instance creation delivers resources and is
    /// nowhere near real-time safe.
    pub fn create(
        plugin: &'plugin LoadedPlugin,
        sample_rate: f64,
        maximum_frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) -> anyhow::Result<Option<Self>> {
        let Some(layout) = plugin.parallel_layout() else {
            return Ok(None);
        };
        let samples = maximum_frames as usize * output_channels as usize;
        let mut cells = Vec::with_capacity(layout.max_units);
        for unit in 0..layout.max_units {
            let mut instance = plugin.create_instance()?;
            if !instance.is_portable() {
                anyhow::bail!("parallel units require the portable wasm-v1 backend");
            }
            instance.activate(sample_rate, maximum_frames, input_channels, output_channels)?;
            cells.push(Box::new(UnitCell {
                unit: unit as u32,
                instance,
                payload: vec![0_u8; layout.dispatch_stride].into_boxed_slice(),
                payload_len: 0,
                output: vec![0.0_f32; samples].into_boxed_slice(),
                input_ptr: std::ptr::null(),
                input_len: 0,
                output_samples: 0,
            }));
        }
        Ok(Some(Self {
            layout,
            cells,
            plan: vec![ParallelPlanEntry::default(); MAX_PARALLEL_UNITS].into_boxed_slice(),
            plan_mask: 0,
            sched_mask: 0,
            quarantined_units: 0,
            maximum_frames,
            input_channels,
            output_channels,
        }))
    }

    pub fn max_units(&self) -> u32 {
        self.layout.max_units as u32
    }

    /// Mirrors a control-plane operation onto every worker instance so unit
    /// state can never depend on which host thread rendered it. Per-block
    /// dynamics still travel exclusively through dispatch payloads.
    pub fn mirror<E>(
        &mut self,
        mut operation: impl FnMut(&mut PluginInstance<'plugin>) -> Result<(), E>,
    ) -> Result<(), E> {
        for cell in &mut self.cells {
            operation(&mut cell.instance)?;
        }
        Ok(())
    }

    /// Re-prepares every worker instance and resizes block buffers after an
    /// audio-profile change. Control/setup thread only.
    pub fn reconfigure(
        &mut self,
        sample_rate: f64,
        maximum_frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) -> anyhow::Result<()> {
        let samples = maximum_frames as usize * output_channels as usize;
        for cell in &mut self.cells {
            cell.instance
                .activate(sample_rate, maximum_frames, input_channels, output_channels)?;
            if cell.output.len() < samples {
                cell.output = vec![0.0_f32; samples].into_boxed_slice();
            }
        }
        self.maximum_frames = maximum_frames;
        self.input_channels = input_channels;
        self.output_channels = output_channels;
        Ok(())
    }

    /// Serial pre-stage: runs the coordinator's `begin_block` and stages
    /// every announced dispatch payload into its unit cell. Returns the
    /// bitmask of units the scheduler should run, with previously
    /// quarantined units already excluded.
    pub fn begin(
        &mut self,
        coordinator: &mut PluginInstance<'plugin>,
        input: &[f32],
        frames: u32,
        midi_events: &[MidiEventV1],
        parameter_events: &[ParameterEventV1],
    ) -> anyhow::Result<u32> {
        let active = coordinator.parallel_begin_block(
            input,
            frames,
            midi_events,
            parameter_events,
            &mut self.plan,
        )?;
        let mut mask = 0_u32;
        for entry in &self.plan[..active] {
            let cell = &mut self.cells[entry.unit as usize];
            let length = entry.payload_bytes as usize;
            coordinator.parallel_read_dispatch(entry.unit, &mut cell.payload[..length])?;
            cell.payload_len = length;
            mask |= 1 << entry.unit;
        }
        self.plan_mask = mask;
        self.sched_mask = mask & !self.quarantined_units;
        Ok(self.sched_mask)
    }

    /// Publishes the pointer table for one unit. The input slice must stay
    /// untouched until the block retires.
    pub fn unit_job(&mut self, unit: u32, input: &[f32], frames: u32, channels: u32) -> UnitJob {
        let cell = &mut self.cells[unit as usize];
        cell.input_ptr = input.as_ptr();
        cell.input_len = input.len();
        cell.output_samples = frames as usize * channels as usize;
        UnitJob {
            context: (&mut **cell as *mut UnitCell<'plugin>).cast(),
            unit,
            run: run_unit_cell,
        }
    }

    /// Serial post-stage: deposits every planned unit into the coordinator's
    /// mix region in ascending unit order — completed units with their audio,
    /// missing ones silenced — then runs `end_block` into `output`. A unit
    /// that failed this block is quarantined so later blocks skip it instead
    /// of burning its fuel budget again.
    pub fn finish(
        &mut self,
        coordinator: &mut PluginInstance<'plugin>,
        output: &mut [f32],
        frames: u32,
        channels: u32,
        completed: u32,
    ) -> anyhow::Result<()> {
        let samples = frames as usize * channels as usize;
        self.quarantined_units |= self.sched_mask & !completed;
        let mut pending = self.plan_mask;
        while pending != 0 {
            let bit = pending.isolate_lowest_one();
            pending &= !bit;
            let unit = bit.trailing_zeros();
            let cell = &mut self.cells[unit as usize];
            if completed & bit == 0 {
                cell.output[..samples].fill(0.0);
            }
            coordinator.parallel_write_mix_slot(unit, &cell.output[..samples])?;
        }
        coordinator.parallel_end_block(output, frames)
    }

    /// Units silenced by earlier faults; diagnostic only.
    pub fn quarantined_units(&self) -> u32 {
        self.quarantined_units
    }

    /// Renders one discarded silent block through the whole unit graph so
    /// code and data pages are touched before the deadline-bound audio loop
    /// takes ownership. Control/setup thread only.
    pub fn warmup(&mut self, coordinator: &mut PluginInstance<'plugin>) -> anyhow::Result<()> {
        let frames = self.maximum_frames;
        let channels = self.output_channels;
        let input = vec![0.0_f32; frames as usize * self.input_channels as usize];
        let mask = self.begin(coordinator, &input, frames, &[], &[])?;
        let mut pending = mask;
        while pending != 0 {
            let bit = pending.isolate_lowest_one();
            pending &= !bit;
            let unit = bit.trailing_zeros();
            let job = self.unit_job(unit, &input, frames, channels);
            // SAFETY: this thread exclusively owns the whole Slot.
            if !unsafe { execute_unit_job(&job, frames, channels) } {
                anyhow::bail!("parallel unit {unit} failed its warmup block");
            }
        }
        let mut output = vec![0.0_f32; frames as usize * channels as usize];
        self.finish(coordinator, &mut output, frames, channels, mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn spin_for(ns: u64) {
        if ns == 0 {
            return;
        }
        let started = Instant::now();
        while (started.elapsed().as_nanos() as u64) < ns {
            std::hint::spin_loop();
        }
    }

    struct MockUnitCell {
        runs: AtomicUsize,
        fail: bool,
        spin_ns: u64,
    }

    unsafe fn run_mock_unit(context: *mut (), _unit: u32, _frames: u32, _channels: u32) -> bool {
        // SAFETY: the scheduler guarantees an exclusive claim per unit.
        let cell = unsafe { &*(context as *const MockUnitCell) };
        spin_for(cell.spin_ns);
        cell.runs.fetch_add(1, Ordering::SeqCst);
        !cell.fail
    }

    // Boxed for the same reason as the real unit cells: jobs hold raw
    // pointers into them.
    #[allow(clippy::vec_box)]
    struct MockSlot {
        max_units: u32,
        active_mask: u32,
        fail_begin: bool,
        single_spin_ns: u64,
        cells: Vec<Box<MockUnitCell>>,
        singles: usize,
        begins: usize,
        ends: usize,
        end_saw_completed: Vec<u32>,
        runs_at_begin: Vec<usize>,
        dependency_violation: bool,
        quarantined: bool,
    }

    impl MockSlot {
        fn single(spin_ns: u64) -> Self {
            Self::new(0, 0, spin_ns, 0)
        }

        fn parallel(max_units: u32, active_mask: u32, unit_spin_ns: u64) -> Self {
            Self::new(max_units, active_mask, 0, unit_spin_ns)
        }

        fn new(max_units: u32, active_mask: u32, single_spin_ns: u64, unit_spin_ns: u64) -> Self {
            Self {
                max_units,
                active_mask,
                fail_begin: false,
                single_spin_ns,
                cells: (0..max_units)
                    .map(|_| {
                        Box::new(MockUnitCell {
                            runs: AtomicUsize::new(0),
                            fail: false,
                            spin_ns: unit_spin_ns,
                        })
                    })
                    .collect(),
                singles: 0,
                begins: 0,
                ends: 0,
                end_saw_completed: Vec::new(),
                runs_at_begin: Vec::new(),
                dependency_violation: false,
                quarantined: false,
            }
        }

        fn unit_runs(&self, unit: u32) -> usize {
            self.cells[unit as usize].runs.load(Ordering::SeqCst)
        }
    }

    // SAFETY: unit state lives in per-unit boxed cells with atomic counters;
    // every stage method is safe to run from any pool thread.
    unsafe impl ScheduledSlot for MockSlot {
        fn max_units(&self) -> u32 {
            self.max_units
        }

        fn run_single(&mut self, _frames: u32, _channels: u32) -> bool {
            spin_for(self.single_spin_ns);
            self.singles += 1;
            true
        }

        fn run_begin(&mut self, _frames: u32, _channels: u32) -> Option<u32> {
            if self.fail_begin {
                return None;
            }
            self.begins += 1;
            self.runs_at_begin = self
                .cells
                .iter()
                .map(|cell| cell.runs.load(Ordering::SeqCst))
                .collect();
            Some(self.active_mask)
        }

        fn unit_job(&mut self, unit: u32, _frames: u32, _channels: u32) -> UnitJob {
            UnitJob {
                context: (&mut *self.cells[unit as usize] as *mut MockUnitCell).cast(),
                unit,
                run: run_mock_unit,
            }
        }

        fn run_end(&mut self, _frames: u32, _channels: u32, completed: u32) -> bool {
            self.ends += 1;
            self.end_saw_completed.push(completed);
            let mut pending = self.active_mask;
            while pending != 0 {
                let bit = pending.isolate_lowest_one();
                pending &= !bit;
                let unit = bit.trailing_zeros() as usize;
                // A final stage must never observe an unfinished unit: every
                // scheduled unit ran exactly once since this block's begin.
                if self.cells[unit].runs.load(Ordering::SeqCst) != self.runs_at_begin[unit] + 1 {
                    self.dependency_violation = true;
                }
            }
            if completed & !self.active_mask != 0 {
                self.dependency_violation = true;
            }
            true
        }

        fn quarantine(&mut self) {
            self.quarantined = true;
        }
    }

    fn drive(pool: &mut RenderPool<MockSlot>, slots: &mut [MockSlot], blocks: usize) {
        for _ in 0..blocks {
            if !pool.process(slots, 128, 2, 1_000_000_000) {
                process_slots_sequential(slots, 128, 2, pool.telemetry());
            }
        }
    }

    #[test]
    fn every_unit_runs_exactly_once_per_block_for_each_worker_count() {
        for workers in [2_usize, 3, 4] {
            let telemetry = RenderTelemetry::new(workers);
            let mut pool = RenderPool::with_workers(workers, telemetry);
            if pool.worker_count() < workers {
                eprintln!("worker spawn shortfall; skipping count {workers}");
                continue;
            }
            let mut slots = vec![
                MockSlot::single(0),
                MockSlot::parallel(5, 0b10111, 0),
                MockSlot::parallel(2, 0b11, 0),
            ];
            let blocks = 200;
            drive(&mut pool, &mut slots, blocks);
            assert_eq!(slots[0].singles, blocks, "workers={workers}");
            for slot in &slots[1..] {
                assert_eq!(slot.begins, blocks, "workers={workers}");
                assert_eq!(slot.ends, blocks, "workers={workers}");
                assert!(!slot.dependency_violation, "workers={workers}");
                let mut pending = slot.active_mask;
                while pending != 0 {
                    let bit = pending.isolate_lowest_one();
                    pending &= !bit;
                    assert_eq!(
                        slot.unit_runs(bit.trailing_zeros()),
                        blocks,
                        "workers={workers}"
                    );
                }
                for completed in &slot.end_saw_completed {
                    assert_eq!(*completed, slot.active_mask, "workers={workers}");
                }
            }
        }
    }

    #[test]
    fn a_failing_unit_is_counted_and_the_final_stage_still_runs() {
        let telemetry = RenderTelemetry::new(2);
        let mut pool = RenderPool::with_workers(2, Arc::clone(&telemetry));
        let mut slots = vec![MockSlot::parallel(3, 0b111, 0), MockSlot::single(0)];
        slots[0].cells[1].fail = true;
        let blocks = 25;
        drive(&mut pool, &mut slots, blocks);
        assert_eq!(slots[0].ends, blocks);
        for completed in &slots[0].end_saw_completed {
            assert_eq!(*completed, 0b101);
        }
        assert!(!slots[0].quarantined);
        assert_eq!(slots[1].singles, blocks);
        let snapshot = telemetry.snapshot_and_reset();
        assert_eq!(snapshot.unit_faults[0], blocks as u64);
        assert_eq!(snapshot.unit_faults[1], 0);
    }

    #[test]
    fn a_faulty_begin_quarantines_only_its_slot() {
        let telemetry = RenderTelemetry::new(2);
        let mut pool = RenderPool::with_workers(2, Arc::clone(&telemetry));
        let mut slots = vec![
            MockSlot::parallel(2, 0b11, 0),
            MockSlot::parallel(2, 0b11, 0),
        ];
        slots[0].fail_begin = true;
        drive(&mut pool, &mut slots, 10);
        assert!(slots[0].quarantined);
        assert_eq!(slots[0].ends, 0);
        assert!(!slots[1].quarantined);
        assert_eq!(slots[1].ends, 10);
        let snapshot = telemetry.snapshot_and_reset();
        assert_eq!(snapshot.slot_faults[0], 10);
    }

    #[test]
    fn many_rapid_blocks_never_deadlock() {
        let telemetry = RenderTelemetry::new(4);
        let mut pool = RenderPool::with_workers(4, telemetry);
        let mut slots: Vec<MockSlot> = (0..MAX_RENDER_SLOTS)
            .map(|index| {
                if index % 2 == 0 {
                    MockSlot::single(0)
                } else {
                    MockSlot::parallel(4, 0b1111, 0)
                }
            })
            .collect();
        drive(&mut pool, &mut slots, 1_000);
        assert_eq!(slots[0].singles, 1_000);
        assert_eq!(slots[1].ends, 1_000);
    }

    #[test]
    fn sequential_fallback_runs_the_same_graph() {
        let telemetry = RenderTelemetry::new(1);
        let mut slots = vec![MockSlot::single(0), MockSlot::parallel(5, 0b11111, 0)];
        for _ in 0..40 {
            process_slots_sequential(&mut slots, 128, 2, &telemetry);
        }
        assert_eq!(slots[0].singles, 40);
        assert_eq!(slots[1].begins, 40);
        assert_eq!(slots[1].ends, 40);
        assert!(!slots[1].dependency_violation);
        for unit in 0..5 {
            assert_eq!(slots[1].unit_runs(unit), 40);
        }
    }

    /// The scenario from the design brief: one five-unit instrument next to
    /// two cheaper ones. The pool must let workers that finish the cheap
    /// instruments take pending units of the expensive one, improving the
    /// worst block against the strictly sequential rendering of the very
    /// same graph.
    #[test]
    fn unbalanced_load_improves_the_worst_block() {
        let cpus = thread::available_parallelism().map_or(1, |count| count.get());
        if cpus < 4 {
            eprintln!("skipping unbalanced-load benchmark: only {cpus} cpus");
            return;
        }
        let unit_ns = 300_000;
        let light_ns = 150_000;
        let blocks = 30;
        let build = || {
            vec![
                MockSlot::parallel(5, 0b11111, unit_ns),
                MockSlot::single(light_ns),
                MockSlot::single(light_ns),
            ]
        };

        let telemetry = RenderTelemetry::new(1);
        let mut slots = build();
        let mut sequential_worst = 0_u64;
        for _ in 0..blocks {
            let started = Instant::now();
            process_slots_sequential(&mut slots, 128, 2, &telemetry);
            sequential_worst = sequential_worst.max(started.elapsed().as_nanos() as u64);
        }

        let telemetry = RenderTelemetry::new(3);
        let mut pool = RenderPool::with_workers(3, telemetry);
        let mut slots = build();
        // Warm the workers so thread startup does not pollute the measure.
        drive(&mut pool, &mut slots, 5);
        let mut pool_worst = 0_u64;
        for _ in 0..blocks {
            let started = Instant::now();
            assert!(pool.process(&mut slots, 128, 2, 1_000_000_000));
            pool_worst = pool_worst.max(started.elapsed().as_nanos() as u64);
        }
        println!(
            "UNBALANCED_BENCH sequential_worst_us={} pool_worst_us={}",
            sequential_worst / 1_000,
            pool_worst / 1_000
        );
        // Sequential: 5×300µs + 2×150µs ≈ 1.8 ms. Three workers with global
        // work claiming should land well under 80% of that even on a busy
        // test machine.
        assert!(
            pool_worst < sequential_worst * 8 / 10,
            "pool {pool_worst}ns vs sequential {sequential_worst}ns"
        );
    }

    #[test]
    fn histogram_percentiles_are_monotonic_and_close() {
        let histogram = Histogram::new();
        for us in 1..=1_000_u64 {
            histogram.record(us * 1_000);
        }
        let snapshot = histogram.drain();
        assert_eq!(snapshot.count, 1_000);
        let p50 = snapshot.percentile_ns(50.0);
        let p95 = snapshot.percentile_ns(95.0);
        let p99 = snapshot.percentile_ns(99.0);
        assert!(p50 <= p95 && p95 <= p99);
        assert!((450_000..=550_000).contains(&p50), "p50={p50}");
        assert!((900_000..=1_000_000).contains(&p95), "p95={p95}");
        assert!(snapshot.max_ns >= p99);
    }

    #[test]
    fn worker_capacity_policy_matches_the_previous_pool() {
        assert_eq!(automatic_audio_worker_capacity(0), 0);
        assert_eq!(automatic_audio_worker_capacity(1), 0);
        assert_eq!(automatic_audio_worker_capacity(2), 2);
        assert_eq!(automatic_audio_worker_capacity(4), 3);
        assert_eq!(automatic_audio_worker_capacity(64), MAX_RENDER_SLOTS);
    }
}
