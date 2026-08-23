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
    if offset < 0 || offset as usize % alignment != 0 {
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
    for (chunk, event) in memory[range].chunks_exact_mut(8).zip(events) {
        chunk.copy_from_slice(&event.packed().to_le_bytes());
    }
}

pub(crate) fn write_parameters(memory: &mut [u8], range: Range<usize>, events: &[ParameterEvent]) {
    for (chunk, event) in memory[range].chunks_exact_mut(16).zip(events) {
        chunk[0..4].copy_from_slice(&event.frame.to_le_bytes());
        chunk[4..8].copy_from_slice(&event.index.to_le_bytes());
        chunk[8..16].copy_from_slice(&event.value.to_le_bytes());
    }
}

pub(crate) fn write_f32(memory: &mut [u8], range: Range<usize>, samples: &[f32]) {
    for (chunk, sample) in memory[range].chunks_exact_mut(4).zip(samples) {
        chunk.copy_from_slice(&sample.to_le_bytes());
    }
}

pub(crate) fn read_f32(memory: &[u8], range: Range<usize>, samples: &mut [f32]) {
    for (sample, chunk) in samples.iter_mut().zip(memory[range].chunks_exact(4)) {
        *sample = f32::from_le_bytes(chunk.try_into().expect("four-byte sample"));
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
