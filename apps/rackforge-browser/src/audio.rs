//! Block rendering for the page's audio callback.
//!
//! A native host pushes audio into a device it opened. A page is the other way
//! round: the `AudioWorklet` asks for a block and expects it immediately. The
//! engine therefore owns its buffers up front, never allocates while rendering,
//! and smooths master changes across a block the way the appliance host does
//! rather than stepping them at a block boundary.

use rackforge_audio_api::{OutputMeter, OutputMeterSnapshot};
use rackforge_core::PluginInstance;
use rackforge_plugin_api::abi::{MidiEventV1, ParameterEventV1};
use rackforge_session_api::{MasterLevel, MasterPan};

/// How long a master level or pan change takes to reach its target. Matches
/// the appliance host, so the same move sounds the same on both.
const MASTER_SMOOTHING_FRAMES: u32 = 256;
/// Live messages waiting for the next block. Beyond this a controller is
/// sending faster than the page can render, and the oldest is dropped.
const MIDI_QUEUE_CAPACITY: usize = 512;
const PARAMETER_QUEUE_CAPACITY: usize = 512;

#[derive(Clone, Copy)]
pub struct RenderRequest {
    pub frames: u32,
}

/// One smoothed control value, ramped per frame towards its target.
struct Smoothed {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl Smoothed {
    fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
        }
    }

    fn set(&mut self, target: f32) {
        self.target = target;
        self.remaining = MASTER_SMOOTHING_FRAMES;
        self.step = (self.target - self.current) / self.remaining as f32;
    }

    fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.current += self.step;
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
            }
        }
        self.current
    }
}

pub struct AudioEngine {
    maximum_frames: u32,
    channels: u32,
    /// Silence handed to instruments, which take no audio input.
    input: Vec<f32>,
    output: Vec<f32>,
    midi: Vec<MidiEventV1>,
    parameters: Vec<ParameterEventV1>,
    level: Smoothed,
    left: Smoothed,
    right: Smoothed,
    output_meter: OutputMeter,
}

impl AudioEngine {
    pub fn new(
        _sample_rate_hz: f64,
        maximum_frames: u32,
        channels: u32,
        level: MasterLevel,
        pan: MasterPan,
    ) -> Self {
        let samples = maximum_frames as usize * channels as usize;
        let (left, right) = pan.balance();
        Self {
            maximum_frames,
            channels,
            input: Vec::new(),
            output: vec![0.0; samples],
            midi: Vec::with_capacity(MIDI_QUEUE_CAPACITY),
            parameters: Vec::with_capacity(PARAMETER_QUEUE_CAPACITY),
            level: Smoothed::new(level.amplitude()),
            left: Smoothed::new(left),
            right: Smoothed::new(right),
            output_meter: OutputMeter::default(),
        }
    }

    pub fn set_master_level(&mut self, level: MasterLevel) {
        self.level.set(level.amplitude());
    }

    pub fn set_master_pan(&mut self, pan: MasterPan) {
        let (left, right) = pan.balance();
        self.left.set(left);
        self.right.set(right);
    }

    /// Drops anything still queued, so switching instruments cannot deliver a
    /// note-off to a voice that never received its note-on.
    pub fn silence(&mut self) {
        self.midi.clear();
        self.parameters.clear();
    }

    pub fn push_midi(&mut self, frame: u32, data: [u8; 3], length: u8) {
        if self.midi.len() >= MIDI_QUEUE_CAPACITY {
            self.midi.remove(0);
        }
        self.midi.push(MidiEventV1 {
            frame: frame.min(self.maximum_frames.saturating_sub(1)),
            length,
            data,
        });
    }

    pub fn push_parameter(&mut self, event: ParameterEventV1) {
        if self.parameters.len() >= PARAMETER_QUEUE_CAPACITY {
            self.parameters.remove(0);
        }
        self.parameters.push(event);
    }

    pub fn render_silence(&mut self, request: RenderRequest) -> &[f32] {
        let samples = self.block_samples(request);
        self.output[..samples].fill(0.0);
        self.midi.clear();
        self.parameters.clear();
        &self.output[..samples]
    }

    pub fn take_output_meter(&self) -> OutputMeterSnapshot {
        self.output_meter.take()
    }

    /// Renders the active instrument, then applies master level and balance.
    ///
    /// A plugin that fails mid-performance yields silence rather than the
    /// previous block's contents: a repeated buffer is far more unpleasant
    /// than a gap.
    pub fn render(&mut self, request: RenderRequest, instance: &mut PluginInstance<'_>) -> &[f32] {
        let frames = request.frames.min(self.maximum_frames);
        let samples = self.block_samples(request);
        self.midi.sort_by_key(|event| event.frame);
        self.midi.retain(|event| event.frame < frames);
        self.parameters.sort_by_key(|event| event.frame);
        self.parameters.retain(|event| event.frame < frames);

        let rendered = instance.process_interleaved(
            &self.input,
            &mut self.output[..samples],
            frames,
            0,
            self.channels,
            &self.midi,
            &self.parameters,
        );
        self.midi.clear();
        self.parameters.clear();
        if rendered.is_err() {
            self.output[..samples].fill(0.0);
            return &self.output[..samples];
        }

        let channels = self.channels as usize;
        for frame in self.output[..samples].chunks_exact_mut(channels) {
            let level = self.level.next();
            let left = self.left.next();
            let right = self.right.next();
            for (channel, sample) in frame.iter_mut().enumerate() {
                let balance = match channel % 2 {
                    0 => left,
                    _ => right,
                };
                *sample *= level * balance;
            }
            self.output_meter
                .observe_stereo(frame[0], frame.get(1).copied().unwrap_or(frame[0]));
        }
        &self.output[..samples]
    }

    fn block_samples(&self, request: RenderRequest) -> usize {
        let frames = request.frames.min(self.maximum_frames) as usize;
        frames * self.channels as usize
    }
}
