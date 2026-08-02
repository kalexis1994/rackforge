#![forbid(unsafe_code)]

mod chorus;
mod exciter;
mod reverb;

pub use chorus::{Chorus, ChorusParameters, PrepareError};
pub use exciter::{Exciter, ExciterParameters};
pub use reverb::{Reverb, ReverbParameters};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StereoFrame {
    pub left: f32,
    pub right: f32,
}

impl StereoFrame {
    pub const fn new(left: f32, right: f32) -> Self {
        Self { left, right }
    }

    pub const fn splat(sample: f32) -> Self {
        Self::new(sample, sample)
    }

    pub fn mono(self) -> f32 {
        (self.left + self.right) * 0.5
    }
}

/// Contract for DSP nodes that can run on RackForge's audio thread.
///
/// Implementations must not allocate, lock, log or perform I/O from
/// `process`. Construction and `reset` happen outside the hot path.
pub trait StereoProcessor {
    fn reset(&mut self);
    fn process(&mut self, input: StereoFrame) -> StereoFrame;
}
