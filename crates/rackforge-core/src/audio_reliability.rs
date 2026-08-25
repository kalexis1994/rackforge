//! Lock-free audio handoff and deterministic dropout recovery.
//!
//! Construction allocates the fixed stereo ring once. Every producer and
//! callback operation after that uses only preallocated memory and atomics: no
//! mutex, channel, sleep, system call, or allocation is present on the audio
//! callback path. The same primitives are therefore usable by Android today
//! and by other render-ahead backends without duplicating recovery behavior.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use thiserror::Error;

/// Bounded single-producer/single-consumer stereo frame queue.
pub struct StereoRenderQueue {
    samples: Box<[UnsafeCell<f32>]>,
    capacity_frames: usize,
    read_frame: AtomicUsize,
    write_frame: AtomicUsize,
    saturated_pushes: AtomicU64,
    invalid_pushes: AtomicU64,
    underrun_callbacks: AtomicU64,
    underrun_frames: AtomicU64,
}

// SAFETY: the queue contract permits exactly one producer and one consumer.
// The producer writes unpublished slots before a release-store of write_frame;
// the consumer reads only acquired slots and release-stores read_frame before
// the producer can reuse them.
unsafe impl Send for StereoRenderQueue {}
unsafe impl Sync for StereoRenderQueue {}

impl StereoRenderQueue {
    /// Allocates the complete handoff buffer before either real-time side runs.
    pub fn new(capacity_frames: usize) -> Result<Self, AudioReliabilityError> {
        if capacity_frames == 0 {
            return Err(AudioReliabilityError::ZeroQueueCapacity);
        }
        let sample_capacity = capacity_frames
            .checked_mul(2)
            .ok_or(AudioReliabilityError::QueueCapacityOverflow)?;
        Ok(Self {
            samples: (0..sample_capacity)
                .map(|_| UnsafeCell::new(0.0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            capacity_frames,
            read_frame: AtomicUsize::new(0),
            write_frame: AtomicUsize::new(0),
            saturated_pushes: AtomicU64::new(0),
            invalid_pushes: AtomicU64::new(0),
            underrun_callbacks: AtomicU64::new(0),
            underrun_frames: AtomicU64::new(0),
        })
    }

    pub fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }

    pub fn queued_frames(&self) -> usize {
        self.write_frame
            .load(Ordering::Acquire)
            .wrapping_sub(self.read_frame.load(Ordering::Acquire))
    }

    /// Publishes complete interleaved stereo frames without waiting.
    ///
    /// `false` means the whole input was rejected. Partial writes are never
    /// exposed, so a saturated producer cannot tear a stereo frame.
    pub fn push(&self, input: &[f32]) -> bool {
        if !input.len().is_multiple_of(2) {
            self.invalid_pushes.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let frames = input.len() / 2;
        let write = self.write_frame.load(Ordering::Relaxed);
        let read = self.read_frame.load(Ordering::Acquire);
        let queued = write.wrapping_sub(read);
        if frames > self.capacity_frames.saturating_sub(queued) {
            self.saturated_pushes.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        for (sample_index, sample) in input.iter().copied().enumerate() {
            let frame = write.wrapping_add(sample_index / 2);
            let channel = sample_index % 2;
            let slot = (frame % self.capacity_frames) * 2 + channel;
            // SAFETY: this producer exclusively owns every unpublished slot.
            unsafe { *self.samples[slot].get() = sample };
        }
        self.write_frame
            .store(write.wrapping_add(frames), Ordering::Release);
        true
    }

    /// Reads as many complete frames as are available and reports underruns.
    ///
    /// The caller owns the unfilled tail and may pass it to
    /// [`StereoDropoutRecovery::conceal`].
    pub fn pop(&self, output: &mut [f32]) -> usize {
        debug_assert!(output.len().is_multiple_of(2));
        let requested_frames = output.len() / 2;
        let read = self.read_frame.load(Ordering::Relaxed);
        let write = self.write_frame.load(Ordering::Acquire);
        let frames = requested_frames.min(write.wrapping_sub(read));
        for (sample_index, output_sample) in output[..frames * 2].iter_mut().enumerate() {
            let frame = read.wrapping_add(sample_index / 2);
            let channel = sample_index % 2;
            let slot = (frame % self.capacity_frames) * 2 + channel;
            // SAFETY: the producer published this slot before write_frame and
            // cannot reuse it until the read cursor below is released.
            *output_sample = unsafe { *self.samples[slot].get() };
        }
        self.read_frame
            .store(read.wrapping_add(frames), Ordering::Release);
        if frames < requested_frames {
            self.underrun_callbacks.fetch_add(1, Ordering::Relaxed);
            self.underrun_frames
                .fetch_add((requested_frames - frames) as u64, Ordering::Relaxed);
        }
        frames
    }

    pub fn snapshot(&self) -> AudioQueueSnapshot {
        AudioQueueSnapshot {
            capacity_frames: self.capacity_frames,
            queued_frames: self.queued_frames(),
            saturated_pushes: self.saturated_pushes.load(Ordering::Relaxed),
            invalid_pushes: self.invalid_pushes.load(Ordering::Relaxed),
            underrun_callbacks: self.underrun_callbacks.load(Ordering::Relaxed),
            underrun_frames: self.underrun_frames.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioQueueSnapshot {
    pub capacity_frames: usize,
    pub queued_frames: usize,
    pub saturated_pushes: u64,
    pub invalid_pushes: u64,
    pub underrun_callbacks: u64,
    pub underrun_frames: u64,
}

/// Click-free bridge across a missing render block and its first replacement.
pub struct StereoDropoutRecovery {
    last_left_bits: AtomicU32,
    last_right_bits: AtomicU32,
    recovery_pending: AtomicBool,
    concealed_callbacks: AtomicU64,
    recovered_callbacks: AtomicU64,
}

impl Default for StereoDropoutRecovery {
    fn default() -> Self {
        Self::new()
    }
}

impl StereoDropoutRecovery {
    pub const fn new() -> Self {
        Self {
            last_left_bits: AtomicU32::new(0.0_f32.to_bits()),
            last_right_bits: AtomicU32::new(0.0_f32.to_bits()),
            recovery_pending: AtomicBool::new(false),
            concealed_callbacks: AtomicU64::new(0),
            recovered_callbacks: AtomicU64::new(0),
        }
    }

    /// Fades the last valid stereo frame to silence inside an unfilled tail.
    pub fn conceal(&self, output: &mut [f32], maximum_fade_frames: usize) {
        debug_assert!(output.len().is_multiple_of(2));
        let frames = output.len() / 2;
        if frames == 0 {
            return;
        }
        let left = f32::from_bits(self.last_left_bits.load(Ordering::Relaxed));
        let right = f32::from_bits(self.last_right_bits.load(Ordering::Relaxed));
        let fade_frames = frames.min(maximum_fade_frames.max(1));
        for (index, frame) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let gain = if index < fade_frames {
                1.0 - (index + 1) as f32 / fade_frames as f32
            } else {
                0.0
            };
            frame[0] = left * gain;
            frame[1] = right * gain;
        }
        self.recovery_pending.store(true, Ordering::Release);
        self.concealed_callbacks.fetch_add(1, Ordering::Relaxed);
    }

    /// Fades in the first complete block after one or more concealed tails.
    pub fn recover(&self, output: &mut [f32], maximum_fade_frames: usize) -> bool {
        debug_assert!(output.len().is_multiple_of(2));
        let frames = output.len() / 2;
        if frames == 0 || !self.recovery_pending.swap(false, Ordering::AcqRel) {
            return false;
        }
        let fade_frames = frames.min(maximum_fade_frames.max(1));
        for (index, frame) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            if index < fade_frames {
                let gain = (index + 1) as f32 / fade_frames as f32;
                frame[0] *= gain;
                frame[1] *= gain;
            }
        }
        self.recovered_callbacks.fetch_add(1, Ordering::Relaxed);
        true
    }

    /// Remembers the post-master frame from which a future gap must fade.
    pub fn remember_last_frame(&self, output: &[f32]) {
        debug_assert!(output.len().is_multiple_of(2));
        if let Some(frame) = output.as_chunks::<2>().0.last() {
            self.last_left_bits
                .store(frame[0].to_bits(), Ordering::Relaxed);
            self.last_right_bits
                .store(frame[1].to_bits(), Ordering::Relaxed);
        }
    }

    pub fn reset(&self) {
        self.last_left_bits
            .store(0.0_f32.to_bits(), Ordering::Relaxed);
        self.last_right_bits
            .store(0.0_f32.to_bits(), Ordering::Relaxed);
        self.recovery_pending.store(false, Ordering::Relaxed);
        self.concealed_callbacks.store(0, Ordering::Relaxed);
        self.recovered_callbacks.store(0, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> DropoutRecoverySnapshot {
        DropoutRecoverySnapshot {
            recovery_pending: self.recovery_pending.load(Ordering::Acquire),
            concealed_callbacks: self.concealed_callbacks.load(Ordering::Relaxed),
            recovered_callbacks: self.recovered_callbacks.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DropoutRecoverySnapshot {
    pub recovery_pending: bool,
    pub concealed_callbacks: u64,
    pub recovered_callbacks: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum AudioStreamHealth {
    Healthy = 0,
    Lost = 1,
    Recovering = 2,
}

/// Atomic lifecycle and counters shared by an error callback and control loop.
pub struct AudioStreamRecovery {
    health: AtomicU8,
    losses: AtomicU64,
    recoveries: AtomicU64,
}

impl Default for AudioStreamRecovery {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioStreamRecovery {
    pub const fn new() -> Self {
        Self {
            health: AtomicU8::new(AudioStreamHealth::Healthy as u8),
            losses: AtomicU64::new(0),
            recoveries: AtomicU64::new(0),
        }
    }

    pub fn mark_lost(&self) {
        let previous = self
            .health
            .swap(AudioStreamHealth::Lost as u8, Ordering::AcqRel);
        if previous != AudioStreamHealth::Lost as u8 {
            self.losses.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn mark_recovering(&self) {
        self.health
            .store(AudioStreamHealth::Recovering as u8, Ordering::Release);
    }

    pub fn mark_healthy(&self) {
        let previous = self
            .health
            .swap(AudioStreamHealth::Healthy as u8, Ordering::AcqRel);
        if previous != AudioStreamHealth::Healthy as u8 {
            self.recoveries.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn reset(&self) {
        self.health
            .store(AudioStreamHealth::Healthy as u8, Ordering::Relaxed);
        self.losses.store(0, Ordering::Relaxed);
        self.recoveries.store(0, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> AudioStreamRecoverySnapshot {
        let health = match self.health.load(Ordering::Acquire) {
            1 => AudioStreamHealth::Lost,
            2 => AudioStreamHealth::Recovering,
            _ => AudioStreamHealth::Healthy,
        };
        AudioStreamRecoverySnapshot {
            health,
            losses: self.losses.load(Ordering::Relaxed),
            recoveries: self.recoveries.load(Ordering::Relaxed),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AudioStreamRecoverySnapshot {
    pub health: AudioStreamHealth,
    pub losses: u64,
    pub recoveries: u64,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AudioReliabilityError {
    #[error("audio render queue capacity must be greater than zero")]
    ZeroQueueCapacity,
    #[error("audio render queue capacity overflows the stereo sample count")]
    QueueCapacityOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_capacities_and_non_stereo_pushes_are_rejected() {
        assert!(matches!(
            StereoRenderQueue::new(0),
            Err(AudioReliabilityError::ZeroQueueCapacity)
        ));
        assert!(matches!(
            StereoRenderQueue::new(usize::MAX),
            Err(AudioReliabilityError::QueueCapacityOverflow)
        ));
        let queue = StereoRenderQueue::new(2).unwrap();
        assert!(!queue.push(&[0.5]));
        assert_eq!(queue.snapshot().invalid_pushes, 1);
    }

    #[test]
    fn saturation_rejects_the_complete_push_without_corrupting_queued_audio() {
        let queue = StereoRenderQueue::new(2).unwrap();
        assert!(queue.push(&[0.1, -0.1, 0.2, -0.2]));
        assert!(!queue.push(&[0.9, -0.9]));
        let mut output = [0.0; 4];
        assert_eq!(queue.pop(&mut output), 2);
        assert_eq!(output, [0.1, -0.1, 0.2, -0.2]);
        assert_eq!(queue.snapshot().saturated_pushes, 1);
    }

    #[test]
    fn partial_reads_report_every_missing_frame_without_waiting() {
        let queue = StereoRenderQueue::new(4).unwrap();
        assert!(queue.push(&[0.25, -0.25]));
        let mut output = [0.0; 6];
        assert_eq!(queue.pop(&mut output), 1);
        let snapshot = queue.snapshot();
        assert_eq!(snapshot.underrun_callbacks, 1);
        assert_eq!(snapshot.underrun_frames, 2);
        assert_eq!(snapshot.queued_frames, 0);
    }

    #[test]
    fn dropout_fades_to_silence_and_recovery_fades_in_once() {
        let recovery = StereoDropoutRecovery::new();
        recovery.remember_last_frame(&[0.8, -0.4]);
        let mut missing = [9.0; 8];
        recovery.conceal(&mut missing, 4);
        assert_eq!(missing[6..], [0.0, -0.0]);
        assert!(missing[0].abs() < 0.8 && missing[0] > 0.0);
        assert!(missing[1].abs() < 0.4 && missing[1] < 0.0);

        let mut restored = [1.0; 8];
        assert!(recovery.recover(&mut restored, 4));
        assert_eq!(restored[0], 0.25);
        assert_eq!(restored[6], 1.0);
        assert!(!recovery.recover(&mut restored, 4));
        assert_eq!(
            recovery.snapshot(),
            DropoutRecoverySnapshot {
                recovery_pending: false,
                concealed_callbacks: 1,
                recovered_callbacks: 1,
            }
        );
    }

    #[test]
    fn repeated_error_callbacks_count_one_loss_until_recovery_begins() {
        let recovery = AudioStreamRecovery::new();
        recovery.mark_lost();
        recovery.mark_lost();
        assert_eq!(
            recovery.snapshot(),
            AudioStreamRecoverySnapshot {
                health: AudioStreamHealth::Lost,
                losses: 1,
                recoveries: 0,
            }
        );
        recovery.mark_recovering();
        recovery.mark_healthy();
        assert_eq!(
            recovery.snapshot(),
            AudioStreamRecoverySnapshot {
                health: AudioStreamHealth::Healthy,
                losses: 1,
                recoveries: 1,
            }
        );
    }

    #[test]
    fn injected_saturation_loss_and_recovery_preserve_finite_stereo_output() {
        let queue = StereoRenderQueue::new(4).unwrap();
        let dropout = StereoDropoutRecovery::new();
        let stream = AudioStreamRecovery::new();

        assert!(queue.push(&[0.2, -0.2, 0.4, -0.4, 0.6, -0.6, 0.8, -0.8]));
        assert!(!queue.push(&[1.0, -1.0]));
        let mut first = [0.0; 12];
        let rendered = queue.pop(&mut first);
        assert_eq!(rendered, 4);
        dropout.remember_last_frame(&first[..rendered * 2]);
        dropout.conceal(&mut first[rendered * 2..], 2);
        assert!(first.iter().all(|sample| sample.is_finite()));
        assert_eq!(first[10..], [0.0, -0.0]);

        stream.mark_lost();
        let mut lost = [0.0; 8];
        assert_eq!(queue.pop(&mut lost), 0);
        dropout.conceal(&mut lost, 4);
        stream.mark_recovering();

        assert!(queue.push(&[1.0; 8]));
        let mut restored = [0.0; 8];
        assert_eq!(queue.pop(&mut restored), 4);
        assert!(dropout.recover(&mut restored, 4));
        dropout.remember_last_frame(&restored);
        stream.mark_healthy();

        assert!(restored.iter().all(|sample| sample.is_finite()));
        assert_eq!(restored[0], 0.25);
        assert_eq!(restored[6], 1.0);
        assert_eq!(queue.snapshot().saturated_pushes, 1);
        assert_eq!(queue.snapshot().underrun_callbacks, 2);
        assert_eq!(queue.snapshot().underrun_frames, 6);
        assert_eq!(stream.snapshot().health, AudioStreamHealth::Healthy);
    }
}
