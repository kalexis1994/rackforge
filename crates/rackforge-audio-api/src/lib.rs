use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use thiserror::Error;

pub const AUDIO_DEVICE_SCHEMA_VERSION: u32 = 1;
pub const AUDIO_OUTPUT_STATE_SCHEMA_VERSION: u32 = 1;
pub const AUDIO_INPUT_STATE_SCHEMA_VERSION: u32 = 1;
/// How many input channels a plugin may declare: a mono or a stereo input.
/// Not how many the host captures -- see `MAX_CAPTURE_CHANNELS`.
pub const MAX_ACTIVE_INPUT_CHANNELS: usize = 2;
/// How many of an interface's inputs the host may capture at once. Each
/// cable from a Rack's audio input then takes one or two of them, so this is
/// the interface's side, not a plugin's.
pub const MAX_CAPTURE_CHANNELS: usize = 32;
/// How many captured inputs `InputMeter` can follow at once. Above what any
/// capture opens today, so widening the capture never outgrows the meter.
pub const MAX_METERED_INPUT_CHANNELS: usize = 64;
pub const COMMON_SAMPLE_RATES: [u32; 8] = [
    32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 384_000,
];

/// One non-persistent, post-master peak measurement.
///
/// Values are linear full-scale amplitudes. `1.0` is 0 dBFS; values above
/// one are intentionally preserved so a UI can report clipping even when the
/// device conversion clamps the actual sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputMeterSnapshot {
    pub left_peak: f32,
    pub right_peak: f32,
}

impl OutputMeterSnapshot {
    pub fn validate(self) -> Result<(), AudioError> {
        if !self.left_peak.is_finite()
            || !self.right_peak.is_finite()
            || self.left_peak < 0.0
            || self.right_peak < 0.0
        {
            return Err(AudioError::InvalidMeterSnapshot);
        }
        Ok(())
    }
}

/// Lock-free peak accumulator shared by an audio callback and the control UI.
///
/// The callback publishes only the greatest non-negative sample seen since
/// the last `take`; the control side atomically drains both channels. No
/// allocation, mutex, system call, or wake-up occurs on the audio thread.
#[derive(Default)]
pub struct OutputMeter {
    left_peak_bits: AtomicU32,
    right_peak_bits: AtomicU32,
}

impl OutputMeter {
    pub const fn new() -> Self {
        Self {
            left_peak_bits: AtomicU32::new(0),
            right_peak_bits: AtomicU32::new(0),
        }
    }

    pub fn observe_stereo(&self, left: f32, right: f32) {
        publish_peak(&self.left_peak_bits, left.abs());
        publish_peak(&self.right_peak_bits, right.abs());
    }

    pub fn observe_interleaved(&self, samples: &[f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let mut left = 0.0_f32;
        let mut right = 0.0_f32;
        for frame in samples.chunks_exact(channels) {
            let left_sample = frame[0].abs();
            let right_sample = frame.get(1).copied().unwrap_or(frame[0]).abs();
            if left_sample.is_finite() {
                left = left.max(left_sample);
            }
            if right_sample.is_finite() {
                right = right.max(right_sample);
            }
        }
        self.observe_stereo(left, right);
    }

    pub fn take(&self) -> OutputMeterSnapshot {
        OutputMeterSnapshot {
            left_peak: f32::from_bits(self.left_peak_bits.swap(0, Ordering::AcqRel)),
            right_peak: f32::from_bits(self.right_peak_bits.swap(0, Ordering::AcqRel)),
        }
    }
}

/// Lock-free peaks of the captured inputs, one per input in the order the
/// host captures them -- what arrives, after the host's input trim and
/// before any cable's. Same contract as `OutputMeter`: the audio thread
/// publishes the greatest sample since the last `take`, the control side
/// drains it, and nothing on the audio thread allocates or waits.
pub struct InputMeter {
    peak_bits: [AtomicU32; MAX_METERED_INPUT_CHANNELS],
}

impl Default for InputMeter {
    fn default() -> Self {
        Self {
            peak_bits: std::array::from_fn(|_| AtomicU32::new(0)),
        }
    }
}

impl InputMeter {
    /// One block of interleaved capture, `channels` wide.
    pub fn observe_interleaved(&self, samples: &[f32], channels: usize) {
        let metered = channels.min(MAX_METERED_INPUT_CHANNELS);
        if metered == 0 {
            return;
        }
        let mut peaks = [0.0_f32; MAX_METERED_INPUT_CHANNELS];
        for frame in samples.chunks_exact(channels) {
            for (peak, sample) in peaks[..metered].iter_mut().zip(frame) {
                let sample = sample.abs();
                if sample.is_finite() && sample > *peak {
                    *peak = sample;
                }
            }
        }
        self.observe_peaks(&peaks[..metered]);
    }

    /// Peaks the caller measured itself, one per captured input in order.
    pub fn observe_peaks(&self, peaks: &[f32]) {
        for (target, peak) in self.peak_bits.iter().zip(peaks) {
            publish_peak(target, peak.abs());
        }
    }

    /// The peaks of the first `channels` inputs since the last take.
    pub fn take(&self, channels: usize) -> Vec<f32> {
        self.peak_bits[..channels.min(MAX_METERED_INPUT_CHANNELS)]
            .iter()
            .map(|bits| f32::from_bits(bits.swap(0, Ordering::AcqRel)))
            .collect()
    }
}

/// Whether the host is listening to an audio input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioInputAvailability {
    /// An input is chosen and open: `captured` is arriving.
    Open,
    /// No input is chosen in the host's audio settings.
    #[default]
    Disabled,
    /// An input is chosen but could not be opened -- unplugged, busy, or it
    /// refused the format. `reason` says which.
    Absent,
    /// This host has no audio capture at all (the browser, a plugin host).
    Unsupported,
}

/// What a host captures, for a screen that routes it: the Rack editor's
/// audio input node and its cables. Transient: `peaks` is drained by every
/// request, like `OutputMeter`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioInputStatus {
    pub availability: AudioInputAvailability,
    /// The interface, by the name its settings show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    /// How many inputs the interface has, numbered from 1; 0 when unknown.
    #[serde(default)]
    pub device_channels: u16,
    /// The inputs the host captures, one-based, in capture order. A cable
    /// that names another input hears silence.
    #[serde(default)]
    pub captured: Vec<u16>,
    /// The host's own trim on everything it captures, in dB.
    #[serde(default)]
    pub gain_db: i8,
    /// Whether this host honours each cable's own inputs and trim. Only an
    /// engine that plays a whole Rack does; the others play one Slot.
    #[serde(default)]
    pub cable_routing: bool,
    /// Linear peaks since the previous request, one per `captured` input.
    #[serde(default)]
    pub peaks: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn publish_peak(target: &AtomicU32, peak: f32) {
    if !peak.is_finite() {
        return;
    }
    // IEEE-754 bit ordering matches numeric ordering for non-negative floats.
    target.fetch_max(peak.to_bits(), Ordering::Relaxed);
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AudioDeviceId(String);

impl AudioDeviceId {
    pub fn new(value: impl Into<String>) -> Result<Self, AudioError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 160
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'-' | b'_' | b'.')
            })
        {
            return Err(AudioError::InvalidDeviceId(value));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AudioDeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioBackend {
    Alsa,
    /// The output a web page renders into. Its settings belong to the browser,
    /// so a host on this backend reports the stream it was given rather than
    /// offering a choice of devices.
    WebAudio,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioTransport {
    Usb,
    BuiltIn,
    Hdmi,
    Unknown,
}

impl AudioTransport {
    /// Where this kind of connection sits when several outputs could serve.
    ///
    /// By kind, never by make: an appliance meets interfaces it was never
    /// told about, and a list of known vendors is a list that is wrong by the
    /// time it ships. What can be said about a device without knowing it:
    ///
    /// - something plugged into USB was plugged in *for this*, by hand, and
    ///   outranks anything soldered to the board;
    /// - a board's own output is a real fallback, quiet but present;
    /// - HDMI carries audio to a screen. On a headless appliance it is
    ///   usually a monitor that is not there, so it goes last;
    /// - an unclassified transport is taken as a wired-in output.
    ///
    /// Whether an output that has just appeared should take over from the
    /// one already playing.
    ///
    /// Only a better kind of connection does. Two interfaces of the same kind
    /// never displace each other, which is the whole rule for an appliance on
    /// a stage: something plugged in while nothing but the board's own output
    /// was available is a player reaching for their interface, and a second
    /// interface plugged in beside a working one is not a request to move the
    /// performance onto it.
    pub fn outranks(self, current: Self) -> bool {
        self.preference() < current.preference()
    }

    /// Lower sorts first.
    pub fn preference(self) -> u8 {
        match self {
            Self::Usb => 0,
            Self::BuiltIn => 1,
            Self::Unknown => 2,
            Self::Hdmi => 3,
        }
    }
}

/// What the watcher found worth acting on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputChange {
    /// The bound output is no longer present.
    Lost,
    /// A better kind of connection appeared while this one plays.
    Better {
        id: AudioDeviceId,
        transport: AudioTransport,
    },
}

/// Decides from one inventory reading, so the decision can be tested without
/// an ALSA card in the machine.
pub fn assess(
    current_id: &AudioDeviceId,
    current_transport: AudioTransport,
    profile: &AudioOutputProfile,
    devices: &[AudioDeviceDescriptor],
) -> Option<OutputChange> {
    if !devices.iter().any(|device| &device.id == current_id) {
        return Some(OutputChange::Lost);
    }
    devices
        .iter()
        .filter(|device| device.transport.outranks(current_transport))
        .filter(|device| profile.validate_against(device).is_ok())
        .min_by(|left, right| {
            left.transport
                .preference()
                .cmp(&right.transport.preference())
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        })
        .map(|device| OutputChange::Better {
            id: device.id.clone(),
            transport: device.transport,
        })
}

/// The output to use when the configuration names none.
///
/// `candidates` are the devices already known to serve the profile. The order
/// is by kind of connection first and by identity second, so the same machine
/// makes the same choice every boot -- an appliance that plays through a
/// different output each time it starts is worse than one that refuses.
pub fn preferred_automatic_output<'a>(
    candidates: &[&'a AudioDeviceDescriptor],
) -> Option<&'a AudioDeviceDescriptor> {
    candidates
        .iter()
        .min_by(|left, right| {
            left.transport
                .preference()
                .cmp(&right.transport.preference())
                .then_with(|| left.id.as_str().cmp(right.id.as_str()))
        })
        .copied()
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioSampleFormat {
    S16Le,
    S24Le,
    S24_3Le,
    S32Le,
    F32Le,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsbAudioIdentity {
    pub vendor_id: u16,
    pub product_id: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
}

impl UsbAudioIdentity {
    pub fn validate(&self) -> Result<(), AudioError> {
        if self.vendor_id == 0 || self.product_id == 0 {
            return Err(AudioError::InvalidUsbIdentity);
        }
        if self
            .serial
            .as_deref()
            .is_some_and(|serial| serial.trim().is_empty() || serial.len() > 128)
        {
            return Err(AudioError::InvalidUsbSerial);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioValueRange {
    pub minimum: u32,
    pub maximum: u32,
}

impl AudioValueRange {
    pub fn new(minimum: u32, maximum: u32) -> Result<Self, AudioError> {
        if minimum == 0 || maximum < minimum {
            return Err(AudioError::InvalidRange { minimum, maximum });
        }
        Ok(Self { minimum, maximum })
    }

    pub const fn contains(self, value: u32) -> bool {
        value >= self.minimum && value <= self.maximum
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioStreamCapabilities {
    pub sample_formats: Vec<AudioSampleFormat>,
    pub sample_rates_hz: Vec<u32>,
    pub channels: AudioValueRange,
    pub period_frames: AudioValueRange,
    pub buffer_frames: AudioValueRange,
}

impl AudioStreamCapabilities {
    pub fn validate(&self) -> Result<(), AudioError> {
        if self.sample_formats.is_empty() {
            return Err(AudioError::EmptyFormats);
        }
        if self.sample_rates_hz.is_empty()
            || self.sample_rates_hz.contains(&0)
            || !strictly_increasing(&self.sample_rates_hz)
        {
            return Err(AudioError::InvalidSampleRates);
        }
        AudioValueRange::new(self.channels.minimum, self.channels.maximum)?;
        AudioValueRange::new(self.period_frames.minimum, self.period_frames.maximum)?;
        AudioValueRange::new(self.buffer_frames.minimum, self.buffer_frames.maximum)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioDeviceDescriptor {
    pub schema_version: u32,
    pub id: AudioDeviceId,
    pub name: String,
    pub backend: AudioBackend,
    pub backend_address: String,
    pub transport: AudioTransport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usb: Option<UsbAudioIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playback: Option<AudioStreamCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<AudioStreamCapabilities>,
}

impl AudioDeviceDescriptor {
    pub fn validate(&self) -> Result<(), AudioError> {
        if self.schema_version != AUDIO_DEVICE_SCHEMA_VERSION {
            return Err(AudioError::UnsupportedSchema(self.schema_version));
        }
        if self.name.trim().is_empty() || self.backend_address.trim().is_empty() {
            return Err(AudioError::EmptyDeviceField);
        }
        if let Some(usb) = &self.usb {
            usb.validate()?;
        }
        if self.transport == AudioTransport::Usb && self.usb.is_none() {
            return Err(AudioError::MissingUsbIdentity);
        }
        if self.playback.is_none() && self.capture.is_none() {
            return Err(AudioError::DeviceWithoutStreams);
        }
        if let Some(playback) = &self.playback {
            playback.validate()?;
        }
        if let Some(capture) = &self.capture {
            capture.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum AudioDeviceSelector {
    /// Whatever output can serve this profile.
    ///
    /// A deployment that is handed to a player rather than configured by one
    /// cannot name the interface in advance: the appliance is imaged once and
    /// the interface is whatever gets plugged into it. Naming a model here --
    /// as the shipped configuration used to -- means every other machine has
    /// no audio until somebody edits a file over SSH.
    ///
    /// Which one is chosen when several can serve is a question about kinds of
    /// device, not about makes: see [`AudioTransport::preference`].
    Automatic,
    Id {
        id: AudioDeviceId,
    },
    Usb {
        vendor_id: u16,
        product_id: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        serial: Option<String>,
    },
}

impl AudioDeviceSelector {
    pub fn validate(&self) -> Result<(), AudioError> {
        if let Self::Usb {
            vendor_id,
            product_id,
            serial,
        } = self
        {
            UsbAudioIdentity {
                vendor_id: *vendor_id,
                product_id: *product_id,
                serial: serial.clone(),
            }
            .validate()?;
        }
        Ok(())
    }

    pub fn matches(&self, device: &AudioDeviceDescriptor) -> bool {
        match self {
            // Automatic names no device, so nothing "is" it. The host picks
            // from what the profile can actually run on instead.
            Self::Automatic => false,
            Self::Id { id } => id == &device.id,
            Self::Usb {
                vendor_id,
                product_id,
                serial,
            } => device.usb.as_ref().is_some_and(|usb| {
                usb.vendor_id == *vendor_id
                    && usb.product_id == *product_id
                    && serial
                        .as_ref()
                        .is_none_or(|expected| usb.serial.as_ref() == Some(expected))
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFallbackPolicy {
    #[default]
    None,
    UniqueCompatible,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioOutputProfile {
    pub device: AudioDeviceSelector,
    #[serde(default)]
    pub fallback: AudioFallbackPolicy,
    pub sample_format: AudioSampleFormat,
    pub sample_rate_hz: u32,
    pub channels: u32,
    pub period_frames: u32,
    pub buffer_frames: u32,
}

/// Persisted capture selection. Physical channel numbers are one-based and
/// ordered: `[2]` exposes input 2 as plugin channel 1, while `[2, 1]` swaps a
/// stereo pair. The host owns this mapping; plugins receive only normalized
/// interleaved samples.
///
/// `channels` left empty -- or left out -- captures every input the
/// interface has (up to `MAX_CAPTURE_CHANNELS`), resolved against the device
/// when it is opened: then each Rack cable chooses its own inputs, and a Rack
/// made on another machine finds its guitar on input 3 wherever there is an
/// input 3. It is also what a USB interface opens most readily, since many
/// offer their whole channel count and nothing less.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioInputProfile {
    pub device: AudioDeviceSelector,
    #[serde(default)]
    pub fallback: AudioFallbackPolicy,
    pub sample_format: AudioSampleFormat,
    pub sample_rate_hz: u32,
    #[serde(default)]
    pub channels: Vec<u32>,
    pub period_frames: u32,
    pub buffer_frames: u32,
    #[serde(default)]
    pub gain_db: i8,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioInputState {
    pub schema_version: u32,
    pub active_device: AudioDeviceDescriptor,
    pub active_profile: AudioInputProfile,
    pub devices: Vec<AudioDeviceDescriptor>,
}

impl AudioInputState {
    pub fn validate(&self) -> Result<(), AudioError> {
        if self.schema_version != AUDIO_INPUT_STATE_SCHEMA_VERSION {
            return Err(AudioError::UnsupportedInputStateSchema(self.schema_version));
        }
        self.active_device.validate()?;
        self.active_profile.validate_against(&self.active_device)?;
        if self.devices.is_empty()
            || !self
                .devices
                .iter()
                .any(|device| device.id == self.active_device.id)
        {
            return Err(AudioError::ActiveDeviceMissing);
        }
        for device in &self.devices {
            device.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioInputDocument {
    pub schema_version: u32,
    pub input: AudioInputProfile,
}

impl AudioInputDocument {
    pub fn new(input: AudioInputProfile) -> Self {
        Self {
            schema_version: AUDIO_INPUT_STATE_SCHEMA_VERSION,
            input,
        }
    }

    pub fn validate(&self) -> Result<(), AudioError> {
        if self.schema_version != AUDIO_INPUT_STATE_SCHEMA_VERSION {
            return Err(AudioError::UnsupportedInputStateSchema(self.schema_version));
        }
        self.input.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioOutputState {
    pub schema_version: u32,
    /// The device carrying the engine, or nothing while it runs with no output
    /// at all -- the state a host boots into when the interface it was
    /// configured for is not plugged in. `active_profile` still says what the
    /// engine renders for, so a device that appears later is adopted without
    /// disturbing a plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_device: Option<AudioDeviceDescriptor>,
    pub active_profile: AudioOutputProfile,
    pub devices: Vec<AudioDeviceDescriptor>,
}

impl AudioOutputState {
    pub fn validate(&self) -> Result<(), AudioError> {
        if self.schema_version != AUDIO_OUTPUT_STATE_SCHEMA_VERSION {
            return Err(AudioError::UnsupportedOutputStateSchema(
                self.schema_version,
            ));
        }
        // With no active device there is nothing to hold the profile against
        // and nothing to look for in the inventory, and an empty inventory is
        // usually the very machine that put the engine in this state. The
        // profile is still checked on its own: it is what the plugins were
        // activated at, and what a device adopted later has to match.
        if let Some(active_device) = &self.active_device {
            active_device.validate()?;
            self.active_profile.validate_against(active_device)?;
            if self.devices.is_empty()
                || !self
                    .devices
                    .iter()
                    .any(|device| device.id == active_device.id)
            {
                return Err(AudioError::ActiveDeviceMissing);
            }
        } else {
            self.active_profile.validate()?;
        }
        for device in &self.devices {
            device.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioOutputDocument {
    pub schema_version: u32,
    pub output: AudioOutputProfile,
}

impl AudioOutputDocument {
    pub fn new(output: AudioOutputProfile) -> Self {
        Self {
            schema_version: AUDIO_OUTPUT_STATE_SCHEMA_VERSION,
            output,
        }
    }

    pub fn validate(&self) -> Result<(), AudioError> {
        if self.schema_version != AUDIO_OUTPUT_STATE_SCHEMA_VERSION {
            return Err(AudioError::UnsupportedOutputStateSchema(
                self.schema_version,
            ));
        }
        self.output.validate()
    }
}

impl AudioOutputProfile {
    pub fn validate(&self) -> Result<(), AudioError> {
        self.device.validate()?;
        if self.sample_rate_hz < 8_000 || self.sample_rate_hz > 768_000 {
            return Err(AudioError::InvalidSampleRate(self.sample_rate_hz));
        }
        if self.channels == 0 || self.channels > 64 {
            return Err(AudioError::InvalidChannelCount(self.channels));
        }
        if self.period_frames < 16 || self.period_frames > 65_536 {
            return Err(AudioError::InvalidPeriod(self.period_frames));
        }
        if self.buffer_frames < self.period_frames * 2
            || self.buffer_frames > self.period_frames.saturating_mul(32)
        {
            return Err(AudioError::InvalidBuffer {
                period: self.period_frames,
                buffer: self.buffer_frames,
            });
        }
        Ok(())
    }

    pub fn validate_against(&self, device: &AudioDeviceDescriptor) -> Result<(), AudioError> {
        self.validate()?;
        device.validate()?;
        let capabilities = device
            .playback
            .as_ref()
            .ok_or_else(|| AudioError::PlaybackUnavailable(device.id.clone()))?;
        if !capabilities.sample_formats.contains(&self.sample_format) {
            return Err(AudioError::UnsupportedFormat(self.sample_format));
        }
        if !capabilities.sample_rates_hz.contains(&self.sample_rate_hz) {
            return Err(AudioError::UnsupportedSampleRate(self.sample_rate_hz));
        }
        if !capabilities.channels.contains(self.channels) {
            return Err(AudioError::UnsupportedChannels(self.channels));
        }
        if !capabilities.period_frames.contains(self.period_frames) {
            return Err(AudioError::UnsupportedPeriod(self.period_frames));
        }
        if !capabilities.buffer_frames.contains(self.buffer_frames) {
            return Err(AudioError::UnsupportedBuffer(self.buffer_frames));
        }
        Ok(())
    }

    pub fn nominal_buffer_latency_ms(&self) -> f64 {
        f64::from(self.buffer_frames) * 1_000.0 / f64::from(self.sample_rate_hz)
    }
}

impl AudioInputProfile {
    pub fn validate(&self) -> Result<(), AudioError> {
        self.device.validate()?;
        if self.sample_rate_hz < 8_000 || self.sample_rate_hz > 768_000 {
            return Err(AudioError::InvalidSampleRate(self.sample_rate_hz));
        }
        if self.channels.len() > MAX_CAPTURE_CHANNELS || self.channels.contains(&0) {
            return Err(AudioError::InvalidInputChannels(self.channels.clone()));
        }
        let mut ordered = self.channels.clone();
        ordered.sort_unstable();
        ordered.dedup();
        if ordered.len() != self.channels.len() {
            return Err(AudioError::InvalidInputChannels(self.channels.clone()));
        }
        if self.period_frames < 16 || self.period_frames > 65_536 {
            return Err(AudioError::InvalidPeriod(self.period_frames));
        }
        if self.buffer_frames < self.period_frames * 2
            || self.buffer_frames > self.period_frames.saturating_mul(32)
        {
            return Err(AudioError::InvalidBuffer {
                period: self.period_frames,
                buffer: self.buffer_frames,
            });
        }
        if !(-60..=24).contains(&self.gain_db) {
            return Err(AudioError::InvalidInputGain(self.gain_db));
        }
        Ok(())
    }

    pub fn validate_against(&self, device: &AudioDeviceDescriptor) -> Result<(), AudioError> {
        self.validate()?;
        device.validate()?;
        let capabilities = device
            .capture
            .as_ref()
            .ok_or_else(|| AudioError::CaptureUnavailable(device.id.clone()))?;
        if !capabilities.sample_formats.contains(&self.sample_format) {
            return Err(AudioError::UnsupportedFormat(self.sample_format));
        }
        if !capabilities.sample_rates_hz.contains(&self.sample_rate_hz) {
            return Err(AudioError::UnsupportedSampleRate(self.sample_rate_hz));
        }
        let stream_channels = if self.captures_every_input() {
            capabilities
                .channels
                .maximum
                .min(MAX_CAPTURE_CHANNELS as u32)
        } else {
            self.channels.iter().copied().max().unwrap_or(0)
        };
        if stream_channels == 0 || !capabilities.channels.contains(stream_channels) {
            return Err(AudioError::UnsupportedChannels(stream_channels));
        }
        if !capabilities.period_frames.contains(self.period_frames) {
            return Err(AudioError::UnsupportedPeriod(self.period_frames));
        }
        if !capabilities.buffer_frames.contains(self.buffer_frames) {
            return Err(AudioError::UnsupportedBuffer(self.buffer_frames));
        }
        Ok(())
    }

    pub fn stream_channels(&self) -> u32 {
        self.channels.iter().copied().max().unwrap_or(0)
    }

    /// Whether this profile captures whatever inputs the interface has,
    /// rather than a list of them.
    pub fn captures_every_input(&self) -> bool {
        self.channels.is_empty()
    }

    /// The profile as a device will be opened with it: every input the
    /// device offers, one to its channel count (capped at
    /// `MAX_CAPTURE_CHANNELS`), when the profile names none; the profile as
    /// it is otherwise. The engine opens only a resolved profile, so what it
    /// captures is always an explicit list.
    pub fn resolved_for(&self, device: &AudioDeviceDescriptor) -> Self {
        if !self.captures_every_input() {
            return self.clone();
        }
        let count = device
            .capture
            .as_ref()
            .map_or(0, |capture| capture.channels.maximum)
            .min(MAX_CAPTURE_CHANNELS as u32);
        Self {
            channels: (1..=count).collect(),
            ..self.clone()
        }
    }

    pub fn nominal_buffer_latency_ms(&self) -> f64 {
        f64::from(self.buffer_frames) * 1_000.0 / f64::from(self.sample_rate_hz)
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum AudioError {
    #[error("output meter peaks must be finite and non-negative")]
    InvalidMeterSnapshot,
    #[error("invalid audio device id {0:?}")]
    InvalidDeviceId(String),
    #[error("unsupported audio schema {0}")]
    UnsupportedSchema(u32),
    #[error("unsupported audio output state schema {0}")]
    UnsupportedOutputStateSchema(u32),
    #[error("unsupported audio input state schema {0}")]
    UnsupportedInputStateSchema(u32),
    #[error("active audio device is missing from the device inventory")]
    ActiveDeviceMissing,
    #[error("invalid physical audio input channels {0:?}")]
    InvalidInputChannels(Vec<u32>),
    #[error("audio input gain {0} dB is outside -60..24 dB")]
    InvalidInputGain(i8),
    #[error("audio device name or backend address is empty")]
    EmptyDeviceField,
    #[error("USB vendor and product ids must be non-zero")]
    InvalidUsbIdentity,
    #[error("USB serial is empty or too long")]
    InvalidUsbSerial,
    #[error("USB transport requires USB identity")]
    MissingUsbIdentity,
    #[error("audio device exposes neither playback nor capture")]
    DeviceWithoutStreams,
    #[error("invalid audio range {minimum}..={maximum}")]
    InvalidRange { minimum: u32, maximum: u32 },
    #[error("audio capabilities expose no sample formats")]
    EmptyFormats,
    #[error("audio sample rates must be non-zero, unique and sorted")]
    InvalidSampleRates,
    #[error("invalid sample rate {0}")]
    InvalidSampleRate(u32),
    #[error("invalid channel count {0}")]
    InvalidChannelCount(u32),
    #[error("invalid period size {0}")]
    InvalidPeriod(u32),
    #[error("buffer {buffer} must be between 2x and 32x period {period}")]
    InvalidBuffer { period: u32, buffer: u32 },
    #[error("device {0} has no playback stream")]
    PlaybackUnavailable(AudioDeviceId),
    #[error("audio capture is unavailable on device {0}")]
    CaptureUnavailable(AudioDeviceId),
    #[error("sample format {0:?} is not supported")]
    UnsupportedFormat(AudioSampleFormat),
    #[error("sample rate {0} is not supported")]
    UnsupportedSampleRate(u32),
    #[error("channel count {0} is not supported")]
    UnsupportedChannels(u32),
    #[error("period size {0} is not supported")]
    UnsupportedPeriod(u32),
    #[error("buffer size {0} is not supported")]
    UnsupportedBuffer(u32),
}

fn strictly_increasing(values: &[u32]) -> bool {
    values.windows(2).all(|window| window[0] < window[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device() -> AudioDeviceDescriptor {
        AudioDeviceDescriptor {
            schema_version: AUDIO_DEVICE_SCHEMA_VERSION,
            id: AudioDeviceId::new("alsa.usb-1235-8211-serial.pcm-0").unwrap(),
            name: "Scarlett Solo USB".into(),
            backend: AudioBackend::Alsa,
            backend_address: "hw:3,0".into(),
            transport: AudioTransport::Usb,
            usb: Some(UsbAudioIdentity {
                vendor_id: 0x1235,
                product_id: 0x8211,
                serial: Some("SERIAL".into()),
            }),
            playback: Some(AudioStreamCapabilities {
                sample_formats: vec![AudioSampleFormat::S32Le],
                sample_rates_hz: vec![44_100, 48_000, 96_000],
                channels: AudioValueRange::new(2, 2).unwrap(),
                period_frames: AudioValueRange::new(32, 8_192).unwrap(),
                buffer_frames: AudioValueRange::new(64, 16_384).unwrap(),
            }),
            capture: Some(AudioStreamCapabilities {
                sample_formats: vec![AudioSampleFormat::S32Le],
                sample_rates_hz: vec![44_100, 48_000, 96_000],
                channels: AudioValueRange::new(1, 2).unwrap(),
                period_frames: AudioValueRange::new(32, 8_192).unwrap(),
                buffer_frames: AudioValueRange::new(64, 16_384).unwrap(),
            }),
        }
    }

    /// A device of a given kind, for asking which kind wins.
    fn output_of(id: &str, transport: AudioTransport) -> AudioDeviceDescriptor {
        AudioDeviceDescriptor {
            id: AudioDeviceId::new(id).unwrap(),
            name: id.into(),
            transport,
            usb: match transport {
                AudioTransport::Usb => Some(UsbAudioIdentity {
                    vendor_id: 0x1234,
                    product_id: 0x5678,
                    serial: None,
                }),
                _ => None,
            },
            capture: None,
            ..device()
        }
    }

    /// The appliance is imaged once and meets the interface later, so the
    /// choice has to be about kinds of connection. A player who plugs an
    /// interface into a Raspberry Pi means to play through it, not through
    /// the jack on the board.
    #[test]
    fn input_meter_keeps_each_input_apart_and_drains_on_take() {
        let meter = InputMeter::default();
        meter.observe_interleaved(&[0.1, -0.5, 0.3, 0.2], 2);
        meter.observe_interleaved(&[-0.4, 0.1, f32::NAN, 0.0], 2);
        assert_eq!(meter.take(2), vec![0.4, 0.5]);
        assert_eq!(meter.take(2), vec![0.0, 0.0]);
    }

    #[test]
    fn input_meter_ignores_what_it_cannot_hold() {
        let meter = InputMeter::default();
        meter.observe_interleaved(&[0.5; 4], 0);
        let wide = vec![0.25_f32; MAX_METERED_INPUT_CHANNELS + 2];
        meter.observe_interleaved(&wide, MAX_METERED_INPUT_CHANNELS + 2);
        let peaks = meter.take(MAX_METERED_INPUT_CHANNELS + 2);
        assert_eq!(peaks.len(), MAX_METERED_INPUT_CHANNELS);
        assert!(peaks.iter().all(|peak| *peak == 0.25));
    }

    #[test]
    fn audio_input_status_round_trips_with_its_wire_names() {
        let status = AudioInputStatus {
            availability: AudioInputAvailability::Open,
            device_name: Some("Scarlett 2i2".into()),
            device_channels: 2,
            captured: vec![1, 2],
            gain_db: 3,
            cable_routing: true,
            peaks: vec![0.5, 0.25],
            reason: None,
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["availability"], "open");
        assert_eq!(
            serde_json::from_value::<AudioInputStatus>(json).unwrap(),
            status
        );
    }

    #[test]
    fn something_plugged_in_outranks_the_board() {
        let built_in = output_of("alsa.card-headphones.pcm-0", AudioTransport::BuiltIn);
        let usb = output_of("alsa.usb-aaaa-bbbb.pcm-0", AudioTransport::Usb);
        let candidates = [&built_in, &usb];
        assert_eq!(
            preferred_automatic_output(&candidates).map(|device| device.id.as_str()),
            Some("alsa.usb-aaaa-bbbb.pcm-0"),
        );
    }

    /// A headless appliance's HDMI usually leads to a screen that is not
    /// there; it is an output of last resort, not a first choice.
    #[test]
    fn hdmi_goes_last() {
        let hdmi = output_of("alsa.card-vc4hdmi0.pcm-0", AudioTransport::Hdmi);
        let built_in = output_of("alsa.card-headphones.pcm-0", AudioTransport::BuiltIn);
        let candidates = [&hdmi, &built_in];
        assert_eq!(
            preferred_automatic_output(&candidates).map(|device| device.id.as_str()),
            Some("alsa.card-headphones.pcm-0"),
        );
    }

    /// Two interfaces of the same kind must not make the machine play
    /// through a different one each boot.
    #[test]
    fn a_tie_is_broken_the_same_way_every_time() {
        let second = output_of("alsa.usb-2222-2222.pcm-0", AudioTransport::Usb);
        let first = output_of("alsa.usb-1111-1111.pcm-0", AudioTransport::Usb);
        assert_eq!(
            preferred_automatic_output(&[&second, &first]).map(|d| d.id.as_str()),
            preferred_automatic_output(&[&first, &second]).map(|d| d.id.as_str()),
        );
        assert_eq!(
            preferred_automatic_output(&[&second, &first]).map(|d| d.id.as_str()),
            Some("alsa.usb-1111-1111.pcm-0"),
        );
    }

    fn plain_output(id: &str, transport: AudioTransport) -> AudioDeviceDescriptor {
        AudioDeviceDescriptor {
            id: AudioDeviceId::new(id).unwrap(),
            name: id.into(),
            transport,
            // A USB transport without a USB identity is not a device the
            // inventory would ever produce, and validation says so.
            usb: match transport {
                AudioTransport::Usb => Some(UsbAudioIdentity {
                    vendor_id: 0x1234,
                    product_id: 0x5678,
                    serial: None,
                }),
                _ => None,
            },
            capture: None,
            ..device()
        }
    }

    fn automatic_profile() -> AudioOutputProfile {
        AudioOutputProfile {
            device: AudioDeviceSelector::Automatic,
            ..profile()
        }
    }

    #[test]
    fn a_steady_machine_asks_for_nothing() {
        let built_in = plain_output("alsa.card-headphones.pcm-0", AudioTransport::BuiltIn);
        let id = built_in.id.clone();
        assert_eq!(
            assess(
                &id,
                AudioTransport::BuiltIn,
                &automatic_profile(),
                &[built_in]
            ),
            None
        );
    }

    #[test]
    fn an_interface_arriving_beside_the_board_is_adopted() {
        let built_in = plain_output("alsa.card-headphones.pcm-0", AudioTransport::BuiltIn);
        let interface = plain_output("alsa.usb-aaaa-bbbb.pcm-0", AudioTransport::Usb);
        let id = built_in.id.clone();
        let change = assess(
            &id,
            AudioTransport::BuiltIn,
            &automatic_profile(),
            &[built_in, interface],
        );
        assert!(matches!(
            change,
            Some(OutputChange::Better {
                transport: AudioTransport::Usb,
                ..
            })
        ));
    }

    /// The rule a stage depends on: a second interface is not a request to
    /// move the performance onto it.
    #[test]
    fn a_second_interface_does_not_move_a_performance() {
        let playing = plain_output("alsa.usb-1111-1111.pcm-0", AudioTransport::Usb);
        let arrived = plain_output("alsa.usb-2222-2222.pcm-0", AudioTransport::Usb);
        let id = playing.id.clone();
        assert_eq!(
            assess(
                &id,
                AudioTransport::Usb,
                &automatic_profile(),
                &[playing, arrived]
            ),
            None
        );
    }

    #[test]
    fn the_output_disappearing_asks_for_a_rebind() {
        let interface = plain_output("alsa.usb-aaaa-bbbb.pcm-0", AudioTransport::Usb);
        let remaining = plain_output("alsa.card-headphones.pcm-0", AudioTransport::BuiltIn);
        assert_eq!(
            assess(
                &interface.id,
                AudioTransport::Usb,
                &automatic_profile(),
                &[remaining]
            ),
            Some(OutputChange::Lost)
        );
    }

    /// An arrival the engine could not play through is no reason to stop
    /// playing: a Raspberry Pi's HDMI and headphone jack both appear as
    /// outputs and neither takes S32_LE.
    #[test]
    fn an_arrival_that_cannot_serve_the_profile_is_ignored() {
        let playing = plain_output("alsa.card-headphones.pcm-0", AudioTransport::BuiltIn);
        let mut unusable = plain_output("alsa.usb-cccc-dddd.pcm-0", AudioTransport::Usb);
        unusable.playback = Some(AudioStreamCapabilities {
            sample_formats: vec![AudioSampleFormat::S16Le],
            sample_rates_hz: vec![48_000],
            channels: AudioValueRange::new(2, 2).unwrap(),
            period_frames: AudioValueRange::new(32, 8_192).unwrap(),
            buffer_frames: AudioValueRange::new(64, 16_384).unwrap(),
        });
        let id = playing.id.clone();
        assert_eq!(
            assess(
                &id,
                AudioTransport::BuiltIn,
                &automatic_profile(),
                &[playing, unusable]
            ),
            None
        );
    }

    #[test]
    fn an_interface_takes_over_from_the_board_but_not_from_another_interface() {
        assert!(AudioTransport::Usb.outranks(AudioTransport::BuiltIn));
        assert!(AudioTransport::Usb.outranks(AudioTransport::Hdmi));
        assert!(
            !AudioTransport::Usb.outranks(AudioTransport::Usb),
            "a second interface must not move a performance off the first"
        );
        assert!(!AudioTransport::BuiltIn.outranks(AudioTransport::Usb));
        assert!(!AudioTransport::Hdmi.outranks(AudioTransport::BuiltIn));
    }

    #[test]
    fn nothing_connected_selects_nothing() {
        assert!(preferred_automatic_output(&[]).is_none());
    }

    /// Automatic names no device, so no device answers to it: the host is
    /// meant to resolve it against what a device can do.
    #[test]
    fn automatic_matches_no_device_by_identity() {
        assert!(!AudioDeviceSelector::Automatic.matches(&device()));
        assert!(AudioDeviceSelector::Automatic.validate().is_ok());
    }

    /// The seed installers ship is read by Core, so the spelling matters.
    #[test]
    fn automatic_is_spelled_the_way_the_configuration_writes_it() {
        let selector: AudioDeviceSelector =
            toml::from_str("mode = \"automatic\"").expect("parsing the automatic selector");
        assert_eq!(selector, AudioDeviceSelector::Automatic);
    }

    fn profile() -> AudioOutputProfile {
        AudioOutputProfile {
            device: AudioDeviceSelector::Usb {
                vendor_id: 0x1235,
                product_id: 0x8211,
                serial: None,
            },
            fallback: AudioFallbackPolicy::None,
            sample_format: AudioSampleFormat::S32Le,
            sample_rate_hz: 48_000,
            channels: 2,
            period_frames: 128,
            buffer_frames: 384,
        }
    }

    fn input_profile() -> AudioInputProfile {
        AudioInputProfile {
            device: profile().device,
            fallback: AudioFallbackPolicy::None,
            sample_format: AudioSampleFormat::S32Le,
            sample_rate_hz: 48_000,
            channels: vec![2],
            period_frames: 128,
            buffer_frames: 384,
            gain_db: 0,
        }
    }

    #[test]
    fn output_meter_keeps_peaks_until_they_are_drained() {
        let meter = OutputMeter::default();
        meter.observe_interleaved(&[0.1, -0.2, -0.8, 0.4], 2);
        meter.observe_stereo(0.3, 1.1);

        assert_eq!(
            meter.take(),
            OutputMeterSnapshot {
                left_peak: 0.8,
                right_peak: 1.1,
            }
        );
        assert_eq!(meter.take(), OutputMeterSnapshot::default());
    }

    #[test]
    fn output_meter_mirrors_mono_and_ignores_non_finite_samples() {
        let meter = OutputMeter::default();
        meter.observe_interleaved(&[f32::NAN, -0.5, f32::INFINITY], 1);
        assert_eq!(
            meter.take(),
            OutputMeterSnapshot {
                left_peak: 0.5,
                right_peak: 0.5,
            }
        );
    }

    #[test]
    fn usb_selector_matches_optional_serial_exactly() {
        let device = device();
        assert!(profile().device.matches(&device));
        let selector = AudioDeviceSelector::Usb {
            vendor_id: 0x1235,
            product_id: 0x8211,
            serial: Some("OTHER".into()),
        };
        assert!(!selector.matches(&device));
    }

    #[test]
    fn validates_profile_against_discovered_capabilities() {
        profile().validate_against(&device()).unwrap();
        let mut invalid = profile();
        invalid.sample_rate_hz = 192_000;
        assert_eq!(
            invalid.validate_against(&device()),
            Err(AudioError::UnsupportedSampleRate(192_000))
        );
    }

    #[test]
    fn rejects_unsafe_buffer_relationships() {
        let mut invalid = profile();
        invalid.buffer_frames = invalid.period_frames;
        assert_eq!(
            invalid.validate(),
            Err(AudioError::InvalidBuffer {
                period: 128,
                buffer: 128
            })
        );
    }

    #[test]
    fn serializes_selectors_without_backend_details() {
        let text = serde_json::to_string(&profile()).unwrap();
        assert!(text.contains("\"mode\":\"usb\""));
        assert!(!text.contains("hw:3,0"));
    }

    #[test]
    fn output_state_requires_active_device_in_inventory() {
        let active = device();
        let state = AudioOutputState {
            schema_version: AUDIO_OUTPUT_STATE_SCHEMA_VERSION,
            active_device: Some(active.clone()),
            active_profile: profile(),
            devices: vec![active],
        };
        state.validate().unwrap();
        let mut missing = state;
        missing.devices.clear();
        assert_eq!(missing.validate(), Err(AudioError::ActiveDeviceMissing));
    }

    #[test]
    fn output_state_without_a_device_is_the_silent_engine() {
        // The state a host boots into with nothing plugged in: no active
        // device, an empty inventory, and a profile that is still the shape
        // the plugins were activated at. Rejecting this is what used to make
        // a missing interface fatal.
        let silent = AudioOutputState {
            schema_version: AUDIO_OUTPUT_STATE_SCHEMA_VERSION,
            active_device: None,
            active_profile: profile(),
            devices: Vec::new(),
        };
        silent.validate().unwrap();

        // An inventory that lists devices none of which fit is the same state:
        // the player has something to choose from, and the engine is carrying
        // none of it.
        let mut offered = silent.clone();
        offered.devices = vec![device()];
        offered.validate().unwrap();

        // The profile is still held to the rules on its own -- it is what a
        // device adopted later has to match.
        let mut broken = silent;
        broken.active_profile.buffer_frames = broken.active_profile.period_frames;
        assert!(broken.validate().is_err());
    }

    #[test]
    fn output_state_omits_the_device_it_does_not_have() {
        // `active_device` is skipped when absent rather than serialised as
        // null, so a reader that predates the silent engine sees a document it
        // still parses under deny_unknown_fields.
        let silent = AudioOutputState {
            schema_version: AUDIO_OUTPUT_STATE_SCHEMA_VERSION,
            active_device: None,
            active_profile: profile(),
            devices: Vec::new(),
        };
        let text = serde_json::to_string(&silent).unwrap();
        assert!(!text.contains("active_device"), "{text}");
        assert_eq!(
            serde_json::from_str::<AudioOutputState>(&text).unwrap(),
            silent
        );
    }

    #[test]
    fn persisted_output_document_round_trips() {
        let document = AudioOutputDocument::new(profile());
        document.validate().unwrap();
        let text = toml::to_string(&document).unwrap();
        assert_eq!(
            toml::from_str::<AudioOutputDocument>(&text).unwrap(),
            document
        );
    }

    #[test]
    fn input_profile_preserves_ordered_physical_channels() {
        let mut profile = input_profile();
        profile.channels = vec![2, 1];
        profile.validate_against(&device()).unwrap();
        assert_eq!(profile.stream_channels(), 2);

        let document = AudioInputDocument::new(profile);
        let text = toml::to_string(&document).unwrap();
        assert_eq!(
            toml::from_str::<AudioInputDocument>(&text).unwrap(),
            document
        );
    }

    #[test]
    fn an_input_profile_without_channels_captures_every_input_the_device_has() {
        let mut profile = input_profile();
        profile.channels = Vec::new();
        assert!(profile.captures_every_input());
        profile.validate().unwrap();
        profile.validate_against(&device()).unwrap();
        let resolved = profile.resolved_for(&device());
        assert_eq!(resolved.channels, vec![1, 2]);
        assert_eq!(resolved.stream_channels(), 2);
        // A named list is left as it is.
        let named = input_profile();
        assert_eq!(named.resolved_for(&device()), named);
    }

    #[test]
    fn capturing_every_input_is_capped_and_needs_a_capture_side() {
        let mut profile = input_profile();
        profile.channels = Vec::new();
        let mut wide = device();
        wide.capture.as_mut().unwrap().channels = AudioValueRange::new(1, 64).unwrap();
        assert_eq!(
            profile.resolved_for(&wide).channels.len(),
            MAX_CAPTURE_CHANNELS
        );
        let mut playback_only = device();
        playback_only.capture = None;
        assert!(profile.resolved_for(&playback_only).channels.is_empty());
        assert!(profile.validate_against(&playback_only).is_err());
    }

    #[test]
    fn an_input_profile_may_name_many_inputs_but_not_more_than_the_host_captures() {
        let mut profile = input_profile();
        profile.channels = (1..=MAX_CAPTURE_CHANNELS as u32).collect();
        profile.validate().unwrap();
        profile.channels.push(MAX_CAPTURE_CHANNELS as u32 + 1);
        assert!(matches!(
            profile.validate(),
            Err(AudioError::InvalidInputChannels(_))
        ));
    }

    #[test]
    fn an_input_profile_left_without_channels_reads_back_as_every_input() {
        let text = r#"
            sample_format = "s32_le"
            sample_rate_hz = 48000
            period_frames = 128
            buffer_frames = 384

            [device]
            mode = "automatic"
        "#;
        let profile: AudioInputProfile = toml::from_str(text).unwrap();
        assert!(profile.captures_every_input());
        profile.validate().unwrap();
    }

    #[test]
    fn input_profile_rejects_duplicates_and_unsafe_gain() {
        let mut profile = input_profile();
        profile.channels = vec![1, 1];
        assert!(matches!(
            profile.validate(),
            Err(AudioError::InvalidInputChannels(_))
        ));
        profile.channels = vec![1];
        profile.gain_db = 25;
        assert_eq!(profile.validate(), Err(AudioError::InvalidInputGain(25)));
    }
}
