use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

pub const AUDIO_DEVICE_SCHEMA_VERSION: u32 = 1;
pub const AUDIO_OUTPUT_STATE_SCHEMA_VERSION: u32 = 1;
pub const AUDIO_INPUT_STATE_SCHEMA_VERSION: u32 = 1;
pub const MAX_ACTIVE_INPUT_CHANNELS: usize = 2;
pub const COMMON_SAMPLE_RATES: [u32; 8] = [
    32_000, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 384_000,
];

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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioInputProfile {
    pub device: AudioDeviceSelector,
    #[serde(default)]
    pub fallback: AudioFallbackPolicy,
    pub sample_format: AudioSampleFormat,
    pub sample_rate_hz: u32,
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
    pub active_device: AudioDeviceDescriptor,
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
        if self.channels.is_empty()
            || self.channels.len() > MAX_ACTIVE_INPUT_CHANNELS
            || self.channels.contains(&0)
        {
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
        let stream_channels = self.channels.iter().copied().max().unwrap_or(0);
        if !capabilities.channels.contains(stream_channels) {
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

    pub fn nominal_buffer_latency_ms(&self) -> f64 {
        f64::from(self.buffer_frames) * 1_000.0 / f64::from(self.sample_rate_hz)
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum AudioError {
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
            active_device: active.clone(),
            active_profile: profile(),
            devices: vec![active],
        };
        state.validate().unwrap();
        let mut missing = state;
        missing.devices.clear();
        assert_eq!(missing.validate(), Err(AudioError::ActiveDeviceMissing));
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
