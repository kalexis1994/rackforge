//! Backend-independent parts of the `wasm-v1` contract.
//!
//! The native backend executes portable components with Wasmtime. The browser
//! backend hands the same components to the engine that is already inside the
//! page. Both agree on the ABI constants, the event encoding and the bounds
//! checks defined here so a plugin cannot behave differently depending on
//! which host is running it.

use anyhow::{Context, Result, bail};
use std::ops::Range;

pub const ABI_VERSION_V1_1: i32 = 0x0001_0001;
pub const ABI_VERSION_V1: i32 = 0x0001_0002;
/// Version reported by `rackforge_parallel_abi_version` for the optional
/// parallel-render extension. Major mismatches are rejected outright.
pub const PARALLEL_ABI_VERSION_V1: i32 = 0x0001_0000;
/// Host-side ceiling on `rackforge_parallel_max_units`. It bounds every
/// preallocated plan, dispatch and mix structure on both sides of the ABI.
pub const MAX_PARALLEL_UNITS: usize = 16;
/// Bytes of one entry in the plan region: `unit: u32` then `payload: u32`.
pub const PARALLEL_PLAN_ENTRY_BYTES: usize = 8;

/// One ready-to-render unit announced by `rackforge_parallel_begin_block`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParallelPlanEntry {
    /// Unit index in `0..max_units`. Entries are strictly increasing, which
    /// both forbids duplicates and fixes the deterministic combine order.
    pub unit: u32,
    /// Bytes of dispatch payload the coordinator wrote for this unit. Never
    /// exceeds the module's declared dispatch stride.
    pub payload_bytes: u32,
}

/// Geometry of a module's parallel-render extension, fixed at instantiation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParallelLayout {
    pub max_units: usize,
    /// Bytes reserved for each unit's dispatch payload slot.
    pub dispatch_stride: usize,
    /// f32 samples reserved for each unit's slot in the mix region.
    pub mix_slot_samples: usize,
}

/// Validates the plan entries a coordinator produced for one block.
pub(crate) fn validate_parallel_plan(
    entries: &[ParallelPlanEntry],
    max_units: usize,
    dispatch_stride: usize,
) -> Result<()> {
    let mut previous: Option<u32> = None;
    for entry in entries {
        if entry.unit as usize >= max_units {
            bail!("parallel plan names unit {} beyond max_units", entry.unit);
        }
        if let Some(previous) = previous
            && entry.unit <= previous
        {
            bail!("parallel plan units must be strictly increasing");
        }
        if entry.payload_bytes as usize > dispatch_stride {
            bail!(
                "parallel plan payload {} exceeds dispatch stride {dispatch_stride}",
                entry.payload_bytes
            );
        }
        previous = Some(entry.unit);
    }
    Ok(())
}
pub(crate) const STATUS_OK: i32 = 0;
pub(crate) const PROGRAM_EDIT_BASIC: u32 = 1 << 0;
pub(crate) const PROGRAM_EDIT_PREVIEW: u32 = 1 << 1;
pub(crate) const PROGRAM_EDIT_DECLARATIVE: u32 = 1 << 2;
pub(crate) const PROGRAM_EDIT_KNOWN_CAPABILITIES: u32 =
    PROGRAM_EDIT_BASIC | PROGRAM_EDIT_PREVIEW | PROGRAM_EDIT_DECLARATIVE;

#[derive(Clone, Copy, Debug)]
pub struct RuntimeLimits {
    pub maximum_memory_bytes: usize,
    /// Fuel available to one real-time audio call.
    pub fuel_per_call: u64,
    /// Fuel available to one bounded control-plane call such as resource
    /// validation or dynamic catalog construction.
    pub control_fuel_per_call: u64,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            maximum_memory_bytes: 64 * 1024 * 1024,
            // Sized against a measured instrument, not a guess. The Concert
            // Grand's worst realistic block — twenty notes under the pedal,
            // its partial budget saturated — bills about 70M fuel while
            // taking ~2.6 ms of a 10.7 ms callback, so the old 50M ceiling
            // aborted blocks that were nowhere near the deadline and took
            // the audio stream down with them. This keeps a real ceiling on
            // a runaway plugin with room for an instrument that works.
            fuel_per_call: 200_000_000,
            control_fuel_per_call: 5_000_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MidiEvent {
    pub frame: u32,
    pub data: [u8; 3],
    pub length: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParameterEvent {
    pub frame: u32,
    pub index: u32,
    pub value: f64,
}

impl MidiEvent {
    pub fn new(frame: u32, data: &[u8]) -> Result<Self> {
        if data.is_empty() || data.len() > 3 {
            bail!("real-time MIDI messages must contain one to three bytes");
        }
        let mut bytes = [0; 3];
        bytes[..data.len()].copy_from_slice(data);
        Ok(Self {
            frame,
            data: bytes,
            length: data.len() as u8,
        })
    }

    pub(crate) fn packed(self) -> u64 {
        self.frame as u64
            | (self.data[0] as u64) << 32
            | (self.data[1] as u64) << 40
            | (self.data[2] as u64) << 48
            | (self.length as u64) << 56
    }
}

pub(crate) fn checked_samples(frames: u32, channels: u32) -> Result<usize> {
    if frames == 0 {
        bail!("frames must be non-zero");
    }
    (frames as usize)
        .checked_mul(channels as usize)
        .context("audio sample count overflow")
}

pub(crate) fn memory_range(
    offset: i32,
    samples: usize,
    memory_size: usize,
) -> Result<Range<usize>> {
    byte_range(
        offset,
        samples,
        size_of::<f32>(),
        align_of::<f32>(),
        memory_size,
    )
}

pub(crate) fn byte_range(
    offset: i32,
    items: usize,
    item_size: usize,
    alignment: usize,
    memory_size: usize,
) -> Result<Range<usize>> {
    if offset < 0 || !(offset as usize).is_multiple_of(alignment) {
        bail!("plugin returned an invalid linear-memory pointer");
    }
    let start = offset as usize;
    let bytes = items
        .checked_mul(item_size)
        .context("buffer byte count overflow")?;
    let end = start
        .checked_add(bytes)
        .context("linear-memory pointer overflow")?;
    if end > memory_size {
        bail!("plugin buffer escapes linear memory");
    }
    Ok(start..end)
}

pub(crate) fn ranges_overlap(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

pub(crate) fn write_midi(memory: &mut [u8], range: Range<usize>, events: &[MidiEvent]) {
    for (chunk, event) in memory[range].as_chunks_mut::<8>().0.iter_mut().zip(events) {
        chunk.copy_from_slice(&event.packed().to_le_bytes());
    }
}

pub(crate) fn write_parameters(memory: &mut [u8], range: Range<usize>, events: &[ParameterEvent]) {
    for (chunk, event) in memory[range].as_chunks_mut::<16>().0.iter_mut().zip(events) {
        chunk[0..4].copy_from_slice(&event.frame.to_le_bytes());
        chunk[4..8].copy_from_slice(&event.index.to_le_bytes());
        chunk[8..16].copy_from_slice(&event.value.to_le_bytes());
    }
}

pub(crate) fn write_f32(memory: &mut [u8], range: Range<usize>, samples: &[f32]) {
    for (chunk, sample) in memory[range].as_chunks_mut::<4>().0.iter_mut().zip(samples) {
        chunk.copy_from_slice(&sample.to_le_bytes());
    }
}

pub(crate) fn read_f32(memory: &[u8], range: Range<usize>, samples: &mut [f32]) {
    for (sample, chunk) in samples.iter_mut().zip(memory[range].as_chunks::<4>().0) {
        *sample = f32::from_le_bytes(*chunk);
    }
}

/// Rejects real-time MIDI and parameter events that would escape the audio
/// block or the capacities the component reported.
pub(crate) fn validate_realtime_events(
    frames: u32,
    midi: &[MidiEvent],
    parameters: &[ParameterEvent],
    capacity_midi_events: usize,
    capacity_parameter_events: usize,
) -> Result<()> {
    if midi.len() > capacity_midi_events {
        bail!("MIDI event count exceeds plugin capacity");
    }
    if parameters.len() > capacity_parameter_events {
        bail!("parameter event count exceeds plugin capacity");
    }
    if midi
        .iter()
        .any(|event| event.frame >= frames || event.length == 0 || event.length > 3)
    {
        bail!("MIDI event is outside the audio block or has an invalid length");
    }
    if parameters
        .iter()
        .any(|event| event.frame >= frames || !event.value.is_finite())
        || parameters
            .windows(2)
            .any(|events| events[0].frame > events[1].frame)
    {
        bail!("parameter events must be finite, ordered, and inside the audio block");
    }
    Ok(())
}

pub(crate) fn check_status(status: i32, operation: &str) -> Result<()> {
    if status == STATUS_OK {
        Ok(())
    } else {
        bail!("portable plugin {operation} failed with status {status}")
    }
}
