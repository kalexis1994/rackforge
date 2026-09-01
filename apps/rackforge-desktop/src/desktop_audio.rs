use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample, SupportedBufferSize};
use keylab_essential_mk3::{controller as keylab_controller, protocol as keylab_protocol};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use rackforge_audio_api::{OutputMeter, OutputMeterSnapshot};
use rackforge_control_api::PluginParameterValue;
use rackforge_core::parallel_render::{
    self, ParallelUnits, RenderPool, RenderTelemetry, ScheduledSlot, UnitJob,
    process_slots_sequential, spawn_telemetry_publisher,
};
use rackforge_core::{
    CompiledParameterLink, LiveParameterStateStore, LiveParameterTarget, LiveParameterWriter,
    LiveParameterWriterHandle, LoadedPlugin, PluginInstance,
    isolated_state::parameter_value_is_valid,
    midi_hotplug::{PanicScope, panic_packets},
};
use rackforge_midi_api::{
    IngressMidiEvent, MidiPacket as RoutedMidiPacket, MidiSourceDescriptor, MidiSourceId,
    MidiSourceKey,
};
use rackforge_plugin_api::{
    ParameterKind, ParameterSchema, PreparedProgram, PresetCatalog,
    abi::{MidiEventV1, ParameterEventV1},
};
use rackforge_session_api::{
    MasterLevel, MasterPan, RackForgeParameterInput, SemanticControlInput,
    rackforge_parameter_input, semantic_control_input,
};
use rackforge_surface_api::{SurfaceActivationRequest, SurfaceActivationResponse};
use rackforge_surface_runtime::{
    Input as SurfaceInput, Screen, ScreenMailbox, ScreenUpdate, SurfaceUpdatePriority,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cell::UnsafeCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const AUDIO_SCHEMA_VERSION: u32 = 1;
const PLUGIN_OUTPUT_CHANNELS: usize = 2;
const MAX_STANDALONE_INPUT_CHANNELS: usize = 2;
const CAPTURE_RING_FRAMES: usize = 16_384;
const MAX_AUDIO_FRAMES: usize = 4_096;
const MIDI_QUEUE_CAPACITY: usize = 4_096;
const COMMAND_QUEUE_CAPACITY: usize = 64;
const CONTROLLER_QUEUE_CAPACITY: usize = 256;
/// Never hand a plugin more MIDI in one block than the ABI guarantees it can
/// take. Every RackForge plugin declares 256 events, and a block that carries
/// more is rejected whole — which killed the audio stream, and killed it again
/// on every retry while the queue kept filling. The surplus stays in the
/// channel and arrives in the next block instead of being dropped: losing a
/// note-off would hang a note forever.
const MAX_MIDI_EVENTS_PER_BLOCK: usize = 256;
const COMMON_SAMPLE_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
const COMMON_BUFFER_FRAMES: [u32; 8] = [32, 64, 128, 256, 512, 1_024, 2_048, 4_096];
const DEFAULT_OUTPUT_GAIN_DB: i8 = 6;
const MAX_OUTPUT_GAIN_DB: i8 = 12;
const DEFAULT_INPUT_GAIN_DB: i8 = 0;
const MIN_INPUT_GAIN_DB: i8 = -60;
const MAX_INPUT_GAIN_DB: i8 = 24;
const MIDI_RECONNECT_INTERVAL: Duration = Duration::from_secs(1);
const MIDI_SUPERVISOR_TICK: Duration = Duration::from_millis(20);
const MASTER_SMOOTHING_FRAMES: u32 = 480;
const CONTROL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const VIRTUAL_MIDI_SOURCE_KEY: MidiSourceKey = MidiSourceKey::new(u32::MAX);

#[derive(Default)]
struct AudioTelemetry {
    callback_count: AtomicU64,
    callback_frames: AtomicU64,
    callback_total_nanos: AtomicU64,
    callback_max_nanos: AtomicU64,
    callback_overruns: AtomicU64,
    midi_dropped_events: AtomicU64,
    midi_panic_count: AtomicU64,
    stream_error_count: AtomicU64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AudioRuntimeStatus {
    pub callback_count: u64,
    pub average_frames: f64,
    pub average_callback_us: f64,
    pub maximum_callback_us: f64,
    pub callback_budget_us: f64,
    pub callback_load_percent: f64,
    pub callback_overruns: u64,
    pub midi_dropped_events: u64,
    pub midi_panic_count: u64,
    pub stream_error_count: u64,
    pub capture_overruns: u64,
    pub capture_underruns: u64,
}

impl AudioTelemetry {
    fn record_callback(&self, frames: usize, sample_rate: u32, elapsed: Duration) {
        let nanos = elapsed.as_nanos().min(u128::from(u64::MAX)) as u64;
        self.callback_count.fetch_add(1, Ordering::Relaxed);
        self.callback_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
        self.callback_total_nanos
            .fetch_add(nanos, Ordering::Relaxed);
        self.callback_max_nanos.fetch_max(nanos, Ordering::Relaxed);
        let budget = (frames as u64)
            .saturating_mul(1_000_000_000)
            .checked_div(u64::from(sample_rate))
            .unwrap_or(0);
        if budget > 0 && nanos > budget {
            self.callback_overruns.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn snapshot(&self, sample_rate: u32) -> AudioRuntimeStatus {
        let callback_count = self.callback_count.load(Ordering::Relaxed);
        let callback_frames = self.callback_frames.load(Ordering::Relaxed);
        let callback_nanos = self.callback_total_nanos.load(Ordering::Relaxed);
        let average_frames = if callback_count == 0 {
            0.0
        } else {
            callback_frames as f64 / callback_count as f64
        };
        let average_callback_us = if callback_count == 0 {
            0.0
        } else {
            callback_nanos as f64 / callback_count as f64 / 1_000.0
        };
        let callback_budget_us = average_frames / f64::from(sample_rate) * 1_000_000.0;
        let callback_load_percent = if callback_budget_us == 0.0 {
            0.0
        } else {
            average_callback_us / callback_budget_us * 100.0
        };
        AudioRuntimeStatus {
            callback_count,
            average_frames,
            average_callback_us,
            maximum_callback_us: self.callback_max_nanos.load(Ordering::Relaxed) as f64 / 1_000.0,
            callback_budget_us,
            callback_load_percent,
            callback_overruns: self.callback_overruns.load(Ordering::Relaxed),
            midi_dropped_events: self.midi_dropped_events.load(Ordering::Relaxed),
            midi_panic_count: self.midi_panic_count.load(Ordering::Relaxed),
            stream_error_count: self.stream_error_count.load(Ordering::Relaxed),
            capture_overruns: 0,
            capture_underruns: 0,
        }
    }
}

/// Bounded single-producer/single-consumer handoff between CPAL's capture and
/// playback callbacks. Neither side allocates, waits or locks. When a driver
/// falls behind, new capture samples are dropped and telemetry makes the
/// failure visible instead of blocking the real-time output callback.
struct CaptureRing {
    samples: Box<[UnsafeCell<f32>]>,
    write: AtomicUsize,
    read: AtomicUsize,
    overruns: AtomicU64,
    underruns: AtomicU64,
}

// SAFETY: there is exactly one capture producer and one playback consumer.
// A slot is written before `write` is released and read only after it is
// acquired. A full slot is never overwritten until the consumer advances.
unsafe impl Sync for CaptureRing {}

impl CaptureRing {
    fn new(capacity: usize) -> Self {
        Self {
            samples: (0..capacity).map(|_| UnsafeCell::new(0.0)).collect(),
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
            overruns: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        }
    }

    fn push_frame(&self, frame: &[f32]) {
        if frame.is_empty() {
            return;
        }
        let write = self.write.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        if write.wrapping_sub(read).saturating_add(frame.len()) > self.samples.len() {
            self.overruns.fetch_add(1, Ordering::Relaxed);
            return;
        }
        for (offset, sample) in frame.iter().copied().enumerate() {
            // SAFETY: this producer exclusively owns every unpublished slot
            // in this complete frame. The write cursor is released once, so
            // the consumer can never observe half a stereo frame.
            unsafe {
                *self.samples[(write + offset) % self.samples.len()].get() = sample;
            }
        }
        self.write
            .store(write.wrapping_add(frame.len()), Ordering::Release);
    }

    fn pop(&self) -> f32 {
        let read = self.read.load(Ordering::Relaxed);
        let write = self.write.load(Ordering::Acquire);
        if read == write {
            self.underruns.fetch_add(1, Ordering::Relaxed);
            return 0.0;
        }
        // SAFETY: the producer published this slot and cannot reuse it until
        // this consumer advances `read` below.
        let sample = unsafe { *self.samples[read % self.samples.len()].get() };
        self.read.store(read.wrapping_add(1), Ordering::Release);
        sample
    }
}

pub struct VoiceSpec {
    pub instance_id: String,
    pub plugin: &'static LoadedPlugin,
    pub preset_id: Option<String>,
    pub resources: BTreeMap<String, PathBuf>,
    /// The player's last live state, restored so the faders mean what they
    /// meant yesterday. Panel edits used to live only in the running
    /// instance and every restart silently reset them.
    pub initial_state: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AudioPreferences {
    pub schema_version: u32,
    pub driver: String,
    pub output_device: String,
    pub sample_rate_hz: u32,
    pub buffer_frames: Option<u32>,
    #[serde(default = "default_output_gain_db")]
    pub output_gain_db: i8,
    /// None keeps capture closed. Audio input is opt-in so connecting a
    /// microphone never starts monitoring it unexpectedly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_device: Option<String>,
    /// One-based physical channel numbers, in the order exposed to plugins.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub input_channels: Vec<u16>,
    #[serde(default = "default_input_gain_db")]
    pub input_gain_db: i8,
    pub midi_inputs: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AudioDriverInfo {
    pub name: String,
    pub available: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AudioOutputInfo {
    pub driver: String,
    pub name: String,
    pub is_default: bool,
    pub channels: u16,
    pub default_sample_rate: u32,
    pub sample_rates: Vec<u32>,
    pub buffer_frames: Vec<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AudioInputInfo {
    pub driver: String,
    pub name: String,
    pub is_default: bool,
    pub channels: u16,
    pub default_sample_rate: u32,
    pub sample_rates: Vec<u32>,
    pub buffer_frames: Vec<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AudioInventory {
    pub drivers: Vec<AudioDriverInfo>,
    pub outputs: Vec<AudioOutputInfo>,
    pub inputs: Vec<AudioInputInfo>,
    pub midi_inputs: Vec<String>,
}

impl AudioInventory {
    pub fn scan() -> Result<Self> {
        Self::scan_skipping(None)
    }

    /// Scans every backend except `skip_driver`. Enumerating instantiates
    /// each ASIO driver, and instantiating the driver that is currently
    /// streaming makes single-client hardware (the Focusrite, measured) stop
    /// the running stream dead. The caller splices the skipped driver's rows
    /// back in from its cache.
    pub fn scan_skipping(skip_driver: Option<&str>) -> Result<Self> {
        let mut drivers = Vec::new();
        let mut outputs = Vec::new();
        let mut inputs = Vec::new();
        for host_id in cpal::available_hosts() {
            if skip_driver == Some(host_id.name()) {
                continue;
            }
            let driver = host_id.name().to_owned();
            let host = match cpal::host_from_id(host_id) {
                Ok(host) => host,
                Err(error) => {
                    drivers.push(AudioDriverInfo {
                        name: driver,
                        available: false,
                        detail: format!("Backend unavailable: {error}"),
                    });
                    continue;
                }
            };
            let default_name = host
                .default_output_device()
                .and_then(|device| device.name().ok());
            let default_input_name = host
                .default_input_device()
                .and_then(|device| device.name().ok());
            let mut driver_outputs = 0usize;
            let mut driver_inputs = 0usize;
            if let Ok(devices) = host.output_devices() {
                for device in devices {
                    let Ok(name) = device.name() else { continue };
                    let Ok(default) = device.default_output_config() else {
                        continue;
                    };
                    let supported = device
                        .supported_output_configs()
                        .map(|configs| configs.collect::<Vec<_>>())
                        .unwrap_or_default();
                    let sample_rates = supported_sample_rates(&supported, default.sample_rate().0);
                    let buffer_frames = supported_buffer_frames(&supported);
                    outputs.push(AudioOutputInfo {
                        driver: driver.clone(),
                        is_default: default_name.as_deref() == Some(name.as_str()),
                        name,
                        channels: default.channels(),
                        default_sample_rate: default.sample_rate().0,
                        sample_rates,
                        buffer_frames,
                    });
                    driver_outputs += 1;
                }
            }
            if let Ok(devices) = host.input_devices() {
                for device in devices {
                    let Ok(name) = device.name() else { continue };
                    let Ok(default) = device.default_input_config() else {
                        continue;
                    };
                    let supported = device
                        .supported_input_configs()
                        .map(|configs| configs.collect::<Vec<_>>())
                        .unwrap_or_default();
                    let sample_rates = supported_sample_rates(&supported, default.sample_rate().0);
                    let buffer_frames = supported_buffer_frames(&supported);
                    inputs.push(AudioInputInfo {
                        driver: driver.clone(),
                        is_default: default_input_name.as_deref() == Some(name.as_str()),
                        name,
                        channels: default.channels(),
                        default_sample_rate: default.sample_rate().0,
                        sample_rates,
                        buffer_frames,
                    });
                    driver_inputs += 1;
                }
            }
            drivers.push(AudioDriverInfo {
                name: driver,
                available: driver_outputs > 0,
                detail: if driver_outputs == 0 {
                    "No output devices found".into()
                } else {
                    format!("{driver_outputs} output device(s) · {driver_inputs} input device(s)")
                },
            });
        }

        if !drivers.iter().any(|driver| driver.name == "ASIO") {
            drivers.push(AudioDriverInfo {
                name: "ASIO".into(),
                available: false,
                detail: "Not included in this build; an ASIO-enabled build and driver are required"
                    .into(),
            });
        }
        outputs.sort_by(|left, right| {
            left.driver
                .cmp(&right.driver)
                .then_with(|| right.is_default.cmp(&left.is_default))
                .then_with(|| left.name.cmp(&right.name))
        });
        inputs.sort_by(|left, right| {
            left.driver
                .cmp(&right.driver)
                .then_with(|| right.is_default.cmp(&left.is_default))
                .then_with(|| left.name.cmp(&right.name))
        });
        let midi_inputs = discover_midi_inputs()?;
        Ok(Self {
            drivers,
            outputs,
            inputs,
            midi_inputs,
        })
    }

    pub fn default_preferences(&self) -> Result<AudioPreferences> {
        let output = self
            .outputs
            .iter()
            .find(|output| output.driver == "WASAPI" && output.is_default)
            .or_else(|| self.outputs.iter().find(|output| output.is_default))
            .or_else(|| self.outputs.iter().find(|output| output.driver == "WASAPI"))
            .or_else(|| self.outputs.first())
            .context("Windows has no available audio output")?;
        Ok(AudioPreferences {
            schema_version: AUDIO_SCHEMA_VERSION,
            driver: output.driver.clone(),
            output_device: output.name.clone(),
            sample_rate_hz: output.default_sample_rate,
            buffer_frames: None,
            output_gain_db: DEFAULT_OUTPUT_GAIN_DB,
            input_device: None,
            input_channels: Vec::new(),
            input_gain_db: DEFAULT_INPUT_GAIN_DB,
            midi_inputs: self.midi_inputs.clone(),
        })
    }

    pub fn output(&self, preferences: &AudioPreferences) -> Option<&AudioOutputInfo> {
        self.outputs.iter().find(|output| {
            output.driver == preferences.driver && output.name == preferences.output_device
        })
    }

    pub fn input(&self, preferences: &AudioPreferences) -> Option<&AudioInputInfo> {
        let name = preferences.input_device.as_deref()?;
        self.inputs
            .iter()
            .find(|input| input.driver == preferences.driver && input.name == name)
    }

    pub fn validate(&self, preferences: &AudioPreferences) -> Result<()> {
        if preferences.schema_version != AUDIO_SCHEMA_VERSION {
            bail!(
                "unsupported audio configuration schema {}",
                preferences.schema_version
            );
        }
        let driver = self
            .drivers
            .iter()
            .find(|driver| driver.name == preferences.driver)
            .with_context(|| format!("audio driver {:?} is not installed", preferences.driver))?;
        if !driver.available {
            bail!("audio driver {:?} is unavailable", preferences.driver);
        }
        let output = self.output(preferences).with_context(|| {
            format!(
                "audio output {:?} is unavailable on {}",
                preferences.output_device, preferences.driver
            )
        })?;
        if !output.sample_rates.contains(&preferences.sample_rate_hz) {
            bail!(
                "{} Hz is not supported by {:?}",
                preferences.sample_rate_hz,
                preferences.output_device
            );
        }
        if let Some(frames) = preferences.buffer_frames
            && !output.buffer_frames.contains(&frames)
        {
            bail!(
                "a {frames}-frame buffer is not supported by {:?}",
                preferences.output_device
            );
        }
        if !(0..=MAX_OUTPUT_GAIN_DB).contains(&preferences.output_gain_db) {
            bail!("output gain must be between 0 and {MAX_OUTPUT_GAIN_DB} dB");
        }
        if !(MIN_INPUT_GAIN_DB..=MAX_INPUT_GAIN_DB).contains(&preferences.input_gain_db) {
            bail!("input gain must be between {MIN_INPUT_GAIN_DB} and {MAX_INPUT_GAIN_DB} dB");
        }
        if let Some(input_name) = preferences.input_device.as_deref() {
            let input = self.input(preferences).with_context(|| {
                format!(
                    "audio input {input_name:?} is unavailable on {}",
                    preferences.driver
                )
            })?;
            if !input.sample_rates.contains(&preferences.sample_rate_hz) {
                bail!(
                    "{} Hz is not supported by input {:?}",
                    preferences.sample_rate_hz,
                    input.name
                );
            }
            if let Some(frames) = preferences.buffer_frames
                && !input.buffer_frames.contains(&frames)
            {
                bail!(
                    "a {frames}-frame buffer is not supported by input {:?}",
                    input.name
                );
            }
            if preferences.input_channels.is_empty() {
                bail!("select at least one channel for audio input {input_name:?}");
            }
            if preferences.input_channels.len() > MAX_STANDALONE_INPUT_CHANNELS {
                bail!(
                    "Desktop supports at most {MAX_STANDALONE_INPUT_CHANNELS} simultaneous input channels"
                );
            }
            let selected = preferences
                .input_channels
                .iter()
                .copied()
                .collect::<BTreeSet<_>>();
            if selected.len() != preferences.input_channels.len()
                || selected
                    .iter()
                    .any(|channel| !(1..=input.channels).contains(channel))
            {
                bail!(
                    "input channels must be unique and between 1 and {} for {:?}",
                    input.channels,
                    input.name
                );
            }
        } else if !preferences.input_channels.is_empty() {
            bail!("audio input channels require an input device");
        }
        Ok(())
    }
}

impl AudioPreferences {
    pub fn load(path: &Path) -> Result<Option<Self>> {
        if !path.is_file() {
            return Ok(None);
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading audio configuration {}", path.display()))?;
        let preferences = toml::from_str(&text)
            .with_context(|| format!("parsing audio configuration {}", path.display()))?;
        Ok(Some(preferences))
    }

    pub fn persist(&self, path: &Path) -> Result<()> {
        let parent = path.parent().context("audio configuration has no parent")?;
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
        if temporary.exists() {
            fs::remove_file(&temporary)
                .with_context(|| format!("removing stale {}", temporary.display()))?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .with_context(|| format!("creating {}", temporary.display()))?;
        file.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if path.exists() {
            fs::remove_file(path).with_context(|| format!("replacing {}", path.display()))?;
        }
        fs::rename(&temporary, path)
            .with_context(|| format!("activating audio configuration {}", path.display()))
    }
}

pub(crate) fn fallback_preserving_midi(
    mut defaults: AudioPreferences,
    saved: &AudioPreferences,
) -> AudioPreferences {
    defaults.midi_inputs.clone_from(&saved.midi_inputs);
    defaults
}

struct PreparedCapture {
    stream: cpal::Stream,
    ring: Arc<CaptureRing>,
    channels: usize,
    summary: String,
}

fn prepare_capture_stream(
    host: &cpal::Host,
    preferences: &AudioPreferences,
    errors: Arc<Mutex<Option<String>>>,
    telemetry: Arc<AudioTelemetry>,
) -> Result<Option<PreparedCapture>> {
    let Some(input_name) = preferences.input_device.as_deref() else {
        return Ok(None);
    };
    if preferences.input_channels.is_empty() {
        bail!("audio input {input_name:?} has no selected channels");
    }
    if preferences.input_channels.len() > MAX_STANDALONE_INPUT_CHANNELS {
        bail!(
            "Desktop supports at most {MAX_STANDALONE_INPUT_CHANNELS} simultaneous input channels"
        );
    }
    if preferences.input_channels.contains(&0) {
        bail!("audio input channels are one-based and cannot contain zero");
    }
    let highest_channel = preferences
        .input_channels
        .iter()
        .copied()
        .max()
        .unwrap_or(0);
    let device = host
        .input_devices()
        .context("enumerating audio inputs")?
        .find(|device| device.name().as_deref() == Ok(input_name))
        .with_context(|| format!("audio input {input_name:?} is unavailable"))?;
    let supported = device
        .supported_input_configs()
        .context("reading supported input formats")?
        .filter(|config| {
            config.channels() >= highest_channel
                && config.min_sample_rate().0 <= preferences.sample_rate_hz
                && preferences.sample_rate_hz <= config.max_sample_rate().0
                && buffer_supported(config.buffer_size(), preferences.buffer_frames)
        })
        .min_by_key(|config| {
            (
                config.sample_format() != SampleFormat::F32,
                config.channels(),
            )
        })
        .with_context(|| {
            format!(
                "{} Hz with the selected buffer and channels is unsupported by input {input_name:?}",
                preferences.sample_rate_hz
            )
        })?;
    let sample_format = supported.sample_format();
    let mut config: cpal::StreamConfig = supported
        .with_sample_rate(cpal::SampleRate(preferences.sample_rate_hz))
        .into();
    config.buffer_size = preferences
        .buffer_frames
        .map_or(BufferSize::Default, BufferSize::Fixed);
    let device_channels = usize::from(config.channels);
    let selected_channels = preferences
        .input_channels
        .iter()
        .map(|channel| usize::from(*channel - 1))
        .collect::<Vec<_>>();
    let exposed_channels = selected_channels.len();
    let ring = Arc::new(CaptureRing::new(
        CAPTURE_RING_FRAMES.saturating_mul(exposed_channels),
    ));
    let callback_ring = Arc::clone(&ring);
    let input_gain = db_to_amplitude(preferences.input_gain_db);
    let callback_errors = Arc::clone(&errors);
    let stream_errors = errors;
    let stream_telemetry = telemetry;
    let stream = device
        .build_input_stream_raw(
            &config,
            sample_format,
            move |data, _| {
                if let Err(error) = capture_input(
                    data,
                    sample_format,
                    device_channels,
                    &selected_channels,
                    input_gain,
                    &callback_ring,
                ) {
                    publish_error(&callback_errors, format!("Audio capture failed: {error:#}"));
                }
            },
            move |error| {
                stream_telemetry
                    .stream_error_count
                    .fetch_add(1, Ordering::Relaxed);
                publish_error(
                    &stream_errors,
                    format!("Audio input stream failed: {error}"),
                );
            },
            None,
        )
        .with_context(|| format!("opening audio input {input_name:?}"))?;
    let selected = preferences
        .input_channels
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join("+");
    Ok(Some(PreparedCapture {
        stream,
        ring,
        channels: exposed_channels,
        summary: format!(
            "input {input_name} ch {selected} {:+} dB",
            preferences.input_gain_db
        ),
    }))
}

pub struct DesktopAudio {
    _stream: cpal::Stream,
    _input_stream: Option<cpal::Stream>,
    _midi_supervisor: MidiSupervisor,
    command_sender: SyncSender<AudioCommand>,
    /// Notes played on a surface, sharing the hardware MIDI queue.
    injected_midi: SyncSender<MidiPacket>,
    controller_receiver: Receiver<DesktopControllerEvent>,
    display_mailbox: ScreenMailbox,
    errors: Arc<Mutex<Option<String>>>,
    telemetry: Arc<AudioTelemetry>,
    output_meter: Arc<OutputMeter>,
    capture_ring: Option<Arc<CaptureRing>>,
    sample_rate: u32,
    summary: String,
    _voice_reclaimer: thread::JoinHandle<()>,
    _live_parameter_writer: LiveParameterWriter,
    data_root: PathBuf,
}

impl DesktopAudio {
    pub fn start(
        specs: Vec<VoiceSpec>,
        preferences: &AudioPreferences,
        active_instance_id: Option<&str>,
        external_controller: bool,
        data_root: &Path,
    ) -> Result<Self> {
        if specs.is_empty() {
            bail!("no playable plugin is available for the audio engine");
        }

        let host_id = cpal::available_hosts()
            .into_iter()
            .find(|host| host.name() == preferences.driver)
            .with_context(|| format!("audio driver {:?} is unavailable", preferences.driver))?;
        let host = cpal::host_from_id(host_id)
            .with_context(|| format!("opening audio driver {}", preferences.driver))?;
        let device = host
            .output_devices()
            .context("enumerating audio outputs")?
            .find(|device| device.name().as_deref() == Ok(preferences.output_device.as_str()))
            .with_context(|| {
                format!(
                    "audio output {:?} is unavailable",
                    preferences.output_device
                )
            })?;
        let supported = device
            .supported_output_configs()
            .context("reading supported audio formats")?
            .filter(|config| {
                config.min_sample_rate().0 <= preferences.sample_rate_hz
                    && preferences.sample_rate_hz <= config.max_sample_rate().0
                    && buffer_supported(config.buffer_size(), preferences.buffer_frames)
            })
            .max_by_key(|config| {
                (
                    config.channels() >= PLUGIN_OUTPUT_CHANNELS as u16,
                    config.sample_format() == SampleFormat::F32,
                    std::cmp::Reverse(config.channels()),
                )
            })
            .with_context(|| {
                format!(
                    "{} Hz with the selected buffer is unsupported by {:?}",
                    preferences.sample_rate_hz, preferences.output_device
                )
            })?;
        let sample_format = supported.sample_format();
        let mut config: cpal::StreamConfig = supported
            .with_sample_rate(cpal::SampleRate(preferences.sample_rate_hz))
            .into();
        config.buffer_size = preferences
            .buffer_frames
            .map_or(BufferSize::Default, BufferSize::Fixed);
        let device_channels = usize::from(config.channels);
        if device_channels == 0 {
            bail!("the selected audio output reports zero channels");
        }

        let live_parameter_store = LiveParameterStateStore::open(Some(data_root))?;
        let live_parameter_targets = specs
            .iter()
            .map(|spec| LiveParameterTarget {
                plugin_id: spec.plugin.manifest().id.clone(),
                plugin_version: spec.plugin.manifest().version.to_string(),
                schema: spec.plugin.parameters().clone(),
            })
            .collect::<Vec<_>>();
        let voice_capacity = specs.len().saturating_add(8);
        let mut voices = Vec::with_capacity(specs.len());
        for (live_parameter_target, spec) in specs.into_iter().enumerate() {
            voices.push(prepare_audio_voice(
                spec,
                config.sample_rate.0,
                live_parameter_target,
                &live_parameter_store,
            )?);
        }
        let live_parameter_writer =
            LiveParameterWriter::start(live_parameter_store, live_parameter_targets);
        let live_parameter_writer_handle = live_parameter_writer.handle();
        let active_voice = active_instance_id
            .and_then(|active| voices.iter().position(|voice| voice.instance_id == active))
            .unwrap_or(0);

        let telemetry = Arc::new(AudioTelemetry::default());
        let output_meter = Arc::new(OutputMeter::default());
        let (midi_sender, midi_receiver) = mpsc::sync_channel(MIDI_QUEUE_CAPACITY);
        // Notes played on a surface deserve the same queue as notes played on
        // a keyboard: 4096 deep, and anything that does not fit in this block
        // waits for the next one.
        let injected_midi = midi_sender.clone();
        let (controller_sender, controller_receiver) =
            mpsc::sync_channel(CONTROLLER_QUEUE_CAPACITY);
        let display_mailbox = ScreenMailbox::default();
        let (midi_supervisor, midi_names) = MidiSupervisor::start(
            midi_sender,
            preferences.midi_inputs.clone(),
            Arc::clone(&telemetry),
            controller_sender,
            display_mailbox.clone(),
            external_controller,
        )?;
        let (command_sender, command_receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let (retired_voice_sender, retired_voice_receiver) = mpsc::sync_channel(voice_capacity);
        let voice_reclaimer = thread::Builder::new()
            .name("rackforge-audio-voice-reclaimer".into())
            .spawn(move || while retired_voice_receiver.recv().is_ok() {})?;
        let errors = Arc::new(Mutex::new(None));
        let prepared_capture = prepare_capture_stream(
            &host,
            preferences,
            Arc::clone(&errors),
            Arc::clone(&telemetry),
        )?;
        let capture_ring = prepared_capture
            .as_ref()
            .map(|capture| Arc::clone(&capture.ring));
        let capture_channels = prepared_capture
            .as_ref()
            .map_or(0, |capture| capture.channels);
        let callback_errors = Arc::clone(&errors);
        let stream_errors = Arc::clone(&errors);
        let callback_telemetry = Arc::clone(&telemetry);
        let stream_telemetry = Arc::clone(&telemetry);
        let callback_sample_rate = config.sample_rate.0;
        let callback_channels = device_channels;
        let (clock_sender, clock_receiver) = mpsc::sync_channel::<u8>(1024);
        spawn_clock_writer(clock_receiver);
        let mut processor = AudioProcessor {
            voices,
            active_voice,
            midi_receiver,
            command_receiver,
            events: Vec::with_capacity(
                MAX_MIDI_EVENTS_PER_BLOCK * (1 + rackforge_core::sequencer::MAX_SEQUENCER_LANES),
            ),
            parameter_events: Vec::with_capacity(MAX_MIDI_EVENTS_PER_BLOCK),
            parameter_links: Vec::new(),
            output: vec![0.0; MAX_AUDIO_FRAMES * PLUGIN_OUTPUT_CHANNELS],
            plugin_input: vec![0.0; MAX_AUDIO_FRAMES * MAX_STANDALONE_INPUT_CHANNELS],
            capture: capture_ring.clone(),
            capture_channels,
            device_channels,
            output_gain: db_to_amplitude(preferences.output_gain_db),
            output_meter: Arc::clone(&output_meter),
            master_gain: MasterGain::new(MasterLevel::UNITY),
            master_balance: MasterBalance::new(MasterPan::CENTER),
            stopped: false,
            conducting: false,
            retired_voice_sender,
            deferred_retire: Vec::with_capacity(voice_capacity),
            live_parameter_writer: live_parameter_writer_handle,
            sample_rate: config.sample_rate.0,
            render_pool: {
                let render_telemetry = RenderTelemetry::new(parallel_render::MAX_RENDER_SLOTS);
                spawn_telemetry_publisher(&render_telemetry, Duration::from_secs(1));
                RenderPool::automatic(render_telemetry)
            },
            render_telemetry: RenderTelemetry::new(0),
            clock_sender,
            clock_scratch: Vec::with_capacity(64),
            sequencer: rackforge_core::SequencerEngine::new(f64::from(config.sample_rate.0))
                .or_else(|| rackforge_core::SequencerEngine::new(48_000.0))
                .expect("48 kHz is inside the transport bounds"),
        };
        processor.render_telemetry = Arc::clone(processor.render_pool.telemetry());
        let engage_callback_thread = std::sync::Once::new();
        let stream = device
            .build_output_stream_raw(
                &config,
                sample_format,
                move |data, _| {
                    // The WASAPI callback thread arrives without the
                    // Multimedia Class boost ASIO drivers provide; request
                    // the best the platform grants, once, on this thread.
                    engage_callback_thread.call_once(|| {
                        let status = rackforge_core::realtime::engage(
                            rackforge_core::realtime::DEFAULT_AUDIO_PRIORITY,
                        );
                        println!("DESKTOP_AUDIO_CALLBACK {status}");
                    });
                    let started = Instant::now();
                    let frames = data.len() / callback_channels;
                    if let Err(error) = render_output(&mut processor, data, sample_format) {
                        silence_output(data, sample_format);
                        // `{:#}` keeps the cause chain: without it every audio
                        // failure reads as the outermost context and the real
                        // reason never reaches the log.
                        publish_error(&callback_errors, format!("{error:#}"));
                    }
                    callback_telemetry.record_callback(
                        frames,
                        callback_sample_rate,
                        started.elapsed(),
                    );
                },
                move |error| {
                    stream_telemetry
                        .stream_error_count
                        .fetch_add(1, Ordering::Relaxed);
                    publish_error(&stream_errors, format!("Audio stream failed: {error}"));
                },
                None,
            )
            .with_context(|| format!("opening audio output {:?}", preferences.output_device))?;
        stream.play().context("starting audio playback")?;
        let (input_stream, input_summary) = if let Some(capture) = prepared_capture {
            capture.stream.play().context("starting audio capture")?;
            (Some(capture.stream), capture.summary)
        } else {
            (None, "input disabled".into())
        };

        let midi_summary = if midi_names.is_empty() {
            "no MIDI inputs".into()
        } else {
            midi_names.join(", ")
        };
        let buffer = preferences.buffer_frames.map_or_else(
            || "default buffer".into(),
            |frames| format!("{frames} frames"),
        );
        let summary = format!(
            "{} · {} · {} Hz · {buffer} · {} ch · {:+} dB · {input_summary} · MIDI: {midi_summary}",
            preferences.driver,
            preferences.output_device,
            config.sample_rate.0,
            config.channels,
            preferences.output_gain_db
        );
        println!("DESKTOP_AUDIO_READY {summary}");
        Ok(Self {
            _stream: stream,
            _input_stream: input_stream,
            _midi_supervisor: midi_supervisor,
            command_sender,
            injected_midi,
            controller_receiver,
            display_mailbox,
            errors,
            telemetry,
            output_meter,
            capture_ring,
            sample_rate: config.sample_rate.0,
            summary,
            _voice_reclaimer: voice_reclaimer,
            _live_parameter_writer: live_parameter_writer,
            data_root: data_root.to_path_buf(),
        })
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn runtime_status(&self) -> AudioRuntimeStatus {
        let mut status = self.telemetry.snapshot(self.sample_rate);
        if let Some(capture) = &self.capture_ring {
            status.capture_overruns = capture.overruns.load(Ordering::Relaxed);
            status.capture_underruns = capture.underruns.load(Ordering::Relaxed);
        }
        status
    }

    pub fn take_output_meter(&self) -> OutputMeterSnapshot {
        self.output_meter.take()
    }

    /// Raw callback count, for the stall watchdog: a healthy stream renders
    /// blocks continuously (silence included), so a counter that stops
    /// advancing means the driver stopped calling back -- which is exactly
    /// how an ASIO device dies when another client grabs the hardware. No
    /// error is ever reported on that path; the count is the only witness.
    pub fn callback_blocks(&self) -> u64 {
        self.telemetry.callback_count.load(Ordering::Relaxed)
    }

    pub fn diagnostics(&self) -> String {
        let status = self.runtime_status();
        format!(
            "{}\nCallback: {} blocks · {:.1}% CPU · avg {:.1} µs · max {:.1} µs · budget {:.1} µs · {:.0} frames\nDeadlines missed: {} · MIDI dropped: {} · disconnect panics: {} · stream errors: {} · input overruns: {} · input underruns: {}",
            self.summary,
            status.callback_count,
            status.callback_load_percent,
            status.average_callback_us,
            status.maximum_callback_us,
            status.callback_budget_us,
            status.average_frames,
            status.callback_overruns,
            status.midi_dropped_events,
            status.midi_panic_count,
            status.stream_error_count,
            status.capture_overruns,
            status.capture_underruns,
        )
    }

    pub fn select_plugin(&self, instance_id: &str) -> Result<()> {
        self.send_command(AudioCommand::SelectPlugin(instance_id.into()))
    }

    pub fn select_sound(&self, instance_id: &str, sound_id: &str) -> Result<()> {
        self.send_command(AudioCommand::SelectSound {
            instance_id: instance_id.into(),
            sound_id: sound_id.into(),
        })
    }

    pub fn activate_surface(
        &self,
        instance_id: &str,
        request: SurfaceActivationRequest,
    ) -> Result<SurfaceActivationResponse> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::ActivateSurface {
            instance_id: instance_id.into(),
            request,
            reply,
        })?;
        receive_control_response(receiver, "activate plugin surface")
    }

    pub fn preview_program(
        &self,
        instance_id: &str,
        prepared: PreparedProgram,
        reset: bool,
    ) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::PreviewProgram {
            instance_id: instance_id.into(),
            prepared,
            reset,
            reply,
        })?;
        receive_control_response(receiver, "preview program")
    }

    pub fn install_program(
        &self,
        instance_id: &str,
        prepared: PreparedProgram,
    ) -> Result<PresetCatalog> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::InstallProgram {
            instance_id: instance_id.into(),
            prepared,
            reply,
        })?;
        receive_control_response(receiver, "install program")
    }

    pub fn restore_program(&self, instance_id: &str, sound_id: Option<&str>) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::RestoreProgram {
            instance_id: instance_id.into(),
            sound_id: sound_id.map(str::to_owned),
            reply,
        })?;
        receive_control_response(receiver, "restore program")
    }

    pub fn plugin_parameters(
        &self,
        instance_id: &str,
    ) -> Result<(ParameterSchema, Vec<PluginParameterValue>)> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::PluginParameters {
            instance_id: instance_id.into(),
            reply,
        })?;
        receive_control_response(receiver, "read plugin parameters")
    }

    pub fn set_plugin_parameter(
        &self,
        instance_id: &str,
        parameter_index: u32,
        value: f64,
    ) -> Result<f64> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::SetPluginParameter {
            instance_id: instance_id.into(),
            parameter_index,
            value,
            reply,
        })?;
        receive_control_response(receiver, "set plugin parameter")
    }

    pub fn replace_parameter_links(&self, links: Vec<CompiledParameterLink>) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::ReplaceParameterLinks { links, reply })?;
        receive_control_response(receiver, "replace parameter links")
    }

    pub fn save_active_state(&self) -> Result<Vec<u8>> {
        let receiver = self.begin_save_active_state()?;
        receive_control_response(receiver, "save plugin state")
    }

    /// Starts a state snapshot without blocking the window thread. Desktop's
    /// close coordinator polls the receiver while continuing to paint its
    /// shutdown progress instead of freezing for the full audio deadline.
    pub fn begin_save_active_state(
        &self,
    ) -> Result<Receiver<std::result::Result<Vec<u8>, String>>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::SaveActiveState { reply })?;
        Ok(receiver)
    }

    pub fn restore_state(&self, instance_id: &str, state: Vec<u8>) -> Result<()> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::RestoreState {
            instance_id: instance_id.into(),
            state,
            reply,
        })?;
        receive_control_response(receiver, "restore plugin state")
    }

    pub fn replace_voice(&self, spec: VoiceSpec) -> Result<()> {
        self._live_parameter_writer.handle().flush();
        let store = LiveParameterStateStore::open(Some(&self.data_root))?;
        let target = self
            ._live_parameter_writer
            .handle()
            .register(LiveParameterTarget {
                plugin_id: spec.plugin.manifest().id.clone(),
                plugin_version: spec.plugin.manifest().version.to_string(),
                schema: spec.plugin.parameters().clone(),
            })
            .context("registering replacement plugin live state")?;
        let voice = prepare_audio_voice(spec, self.sample_rate, target, &store)?;
        self.send_command(AudioCommand::ReplaceVoice(voice))
    }

    pub fn set_master_level(&self, level: MasterLevel) -> Result<()> {
        self.send_command(AudioCommand::SetMasterLevel(level))
    }

    pub fn set_master_pan(&self, pan: MasterPan) -> Result<()> {
        self.send_command(AudioCommand::SetMasterPan(pan))
    }

    pub fn set_running(&self, running: bool) -> Result<()> {
        self.send_command(AudioCommand::SetRunning(running))
    }

    /// Tells the audio thread whether the keyboard is conducting LIVE lanes
    /// or simply playing the instrument.
    pub fn set_conducting(&self, conducting: bool) -> Result<()> {
        self.send_command(AudioCommand::SetConducting(conducting))
    }

    pub fn emergency_stop(&self) -> Result<()> {
        self.send_command(AudioCommand::EmergencyStop)
    }

    pub fn try_controller_event(&self) -> Option<DesktopControllerEvent> {
        self.controller_receiver.try_recv().ok()
    }

    pub fn render_little(&self, screen: Screen) {
        // A controller transport may be slower than the UI. The shared
        // mailbox replaces obsolete pending screens instead of dropping the
        // newest update or blocking the caller.
        self.display_mailbox
            .publish(screen, SurfaceUpdatePriority::Interactive);
    }

    pub fn sequencer_command(
        &self,
        command: rackforge_control_api::SequencerCommand,
    ) -> Result<std::result::Result<(), String>> {
        let (reply, response) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::Sequencer { command, reply })?;
        response
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| anyhow::anyhow!("the audio thread did not answer the sequencer command"))
    }

    pub fn sequencer_capture_take(
        &self,
        lane: u8,
    ) -> Result<Vec<rackforge_control_api::CapturedNoteV1>> {
        let (reply, response) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::SequencerCaptureTake { lane, reply })?;
        response
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| anyhow::anyhow!("the audio thread did not answer the capture take"))
    }

    pub fn sequencer_status(&self) -> Result<rackforge_control_api::SequencerStatusV1> {
        let (reply, response) = mpsc::sync_channel(1);
        self.send_command(AudioCommand::SequencerStatus { reply })?;
        response
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| anyhow::anyhow!("the audio thread did not answer the status request"))
    }

    pub fn test_note(&self) -> Result<()> {
        self.send_command(AudioCommand::InjectMidi(MidiPacket {
            source: VIRTUAL_MIDI_SOURCE_KEY,
            length: 3,
            data: [0x90, 60, 100],
            wide: None,
        }))?;
        let sender = self.command_sender.clone();
        thread::Builder::new()
            .name("rackforge-audio-test-note".into())
            .spawn(move || {
                thread::sleep(Duration::from_millis(350));
                let _ = sender.try_send(AudioCommand::InjectMidi(MidiPacket {
                    source: VIRTUAL_MIDI_SOURCE_KEY,
                    length: 3,
                    data: [0x80, 60, 0],
                    wide: None,
                }));
            })?;
        Ok(())
    }

    /// The queue notes are injected into, so a surface can reach the audio
    /// thread directly instead of waiting for a GUI frame.
    pub fn injected_midi_sender(&self) -> SyncSender<MidiPacket> {
        self.injected_midi.clone()
    }

    pub fn inject_midi_messages_from(
        &self,
        source: MidiSourceKey,
        messages: Vec<[u8; 3]>,
    ) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }
        // The command queue holds 64 entries and rejects what does not fit,
        // so a fast glissando on a touch surface overflowed it and the
        // rejected messages were simply lost. Losing a note-on is a missing
        // note; losing a note-off is a note that hangs until something else
        // damps it — which is what "the last notes keep ringing" was.
        for data in messages {
            self.injected_midi
                .try_send(MidiPacket {
                    source,
                    length: 3,
                    data,
                    wide: None,
                })
                .map_err(|error| {
                    anyhow::anyhow!("MIDI queue rejected an injected note: {error}")
                })?;
        }
        Ok(())
    }

    pub fn take_error(&self) -> Option<String> {
        self.errors.lock().ok()?.take()
    }

    fn send_command(&self, command: AudioCommand) -> Result<()> {
        match self.command_sender.try_send(command) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err(anyhow::anyhow!(
                "audio command queue rejected command: queue full"
            )),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                // A closed channel is proof the engine died without reporting
                // (the silent ASIO stall): raise the flag the recovery loop
                // already watches, so the restart begins on the next frame
                // instead of waiting out the watchdog, and tell the caller
                // something a person can act on.
                if let Ok(mut slot) = self.errors.lock()
                    && slot.is_none()
                {
                    *slot = Some("the audio engine stopped responding".into());
                }
                Err(anyhow::anyhow!(
                    "the audio engine stopped and is being restarted — try again in a moment"
                ))
            }
        }
    }
}

enum AudioCommand {
    SelectPlugin(String),
    SaveActiveState {
        reply: SyncSender<std::result::Result<Vec<u8>, String>>,
    },
    RestoreState {
        instance_id: String,
        state: Vec<u8>,
        reply: SyncSender<std::result::Result<(), String>>,
    },
    SelectSound {
        instance_id: String,
        sound_id: String,
    },
    ActivateSurface {
        instance_id: String,
        request: SurfaceActivationRequest,
        reply: SyncSender<Result<SurfaceActivationResponse, String>>,
    },
    PreviewProgram {
        instance_id: String,
        prepared: PreparedProgram,
        reset: bool,
        reply: SyncSender<Result<(), String>>,
    },
    InstallProgram {
        instance_id: String,
        prepared: PreparedProgram,
        reply: SyncSender<Result<PresetCatalog, String>>,
    },
    RestoreProgram {
        instance_id: String,
        sound_id: Option<String>,
        reply: SyncSender<Result<(), String>>,
    },
    PluginParameters {
        instance_id: String,
        reply: SyncSender<Result<(ParameterSchema, Vec<PluginParameterValue>), String>>,
    },
    SetPluginParameter {
        instance_id: String,
        parameter_index: u32,
        value: f64,
        reply: SyncSender<Result<f64, String>>,
    },
    ReplaceParameterLinks {
        links: Vec<CompiledParameterLink>,
        reply: SyncSender<Result<(), String>>,
    },
    InjectMidi(MidiPacket),
    Sequencer {
        command: rackforge_control_api::SequencerCommand,
        reply: SyncSender<std::result::Result<(), String>>,
    },
    SequencerStatus {
        reply: SyncSender<rackforge_control_api::SequencerStatusV1>,
    },
    SequencerCaptureTake {
        lane: u8,
        reply: SyncSender<Vec<rackforge_control_api::CapturedNoteV1>>,
    },
    SetMasterLevel(MasterLevel),
    SetMasterPan(MasterPan),
    SetRunning(bool),
    SetConducting(bool),
    EmergencyStop,
    ReplaceVoice(AudioVoice),
}

fn receive_control_response<T>(receiver: Receiver<Result<T, String>>, action: &str) -> Result<T> {
    match receiver.recv_timeout(CONTROL_RESPONSE_TIMEOUT) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => bail!("audio engine could not {action}: {error}"),
        Err(RecvTimeoutError::Timeout) => {
            bail!("audio engine did not {action} within the control deadline")
        }
        Err(RecvTimeoutError::Disconnected) => {
            bail!("audio engine disconnected while trying to {action}")
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesktopControllerEvent {
    Connected,
    Disconnected,
    Surface {
        input: SurfaceInput,
        phase: keylab_protocol::InputPhase,
    },
    RackForgeParameter(RackForgeParameterInput),
    SemanticControl(SemanticControlInput),
    MidiObserved {
        source: MidiSourceKey,
        length: u8,
        data: [u8; 3],
        observed_at: Instant,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct MidiPacket {
    pub(crate) source: MidiSourceKey,
    pub(crate) length: u8,
    pub(crate) data: [u8; 3],
    /// The value at MIDI 2.0 width, from a transport that has one.
    pub(crate) wide: Option<u32>,
}

impl MidiPacket {
    fn ingress(self) -> IngressMidiEvent {
        IngressMidiEvent {
            source: self.source,
            packet: RoutedMidiPacket {
                frame: 0,
                length: self.length,
                data: self.data,
                wide: self.wide,
            },
        }
    }
}

struct AudioVoice {
    instance_id: String,
    parameters: ParameterSchema,
    input_channels: usize,
    output_channels: usize,
    instance: SendablePluginInstance,
    /// Host-owned unit instances for `parallel_render_v1` plugins, so the
    /// desktop's PLAY path schedules units across the shared worker pool.
    parallel: Option<SendableParallelUnits>,
    live_parameter_target: usize,
    input: Vec<f32>,
    output: Vec<f32>,
    events: Vec<MidiEventV1>,
    parameter_events: Vec<ParameterEventV1>,
    process_faulted: bool,
}

impl AudioVoice {
    /// Applies one control-plane operation to the coordinator and mirrors
    /// the identical canonical input to every unit instance.
    fn mirror_control<E>(
        &mut self,
        mut operation: impl FnMut(&mut PluginInstance<'static>) -> std::result::Result<(), E>,
    ) -> std::result::Result<(), E> {
        operation(&mut self.instance.0)?;
        if let Some(parallel) = self.parallel.as_mut() {
            parallel.0.mirror(operation)?;
        }
        Ok(())
    }
}

/// One PLAY voice as the shared scheduler sees it.
//
// SAFETY: the same argument as the embedded host — instances reach pool
// workers only under the pool's epoch protocol, and unit jobs point at
// per-unit boxed cells holding isolated portable instances.
unsafe impl ScheduledSlot for AudioVoice {
    fn max_units(&self) -> u32 {
        self.parallel
            .as_ref()
            .map_or(0, |parallel| parallel.0.max_units())
    }

    fn run_single(&mut self, frames: u32, channels: u32) -> bool {
        let samples = frames as usize * channels as usize;
        self.output[..samples].fill(0.0);
        if self.process_faulted {
            return true;
        }
        let input_samples = frames as usize * self.input_channels;
        self.instance
            .0
            .process_interleaved(
                &self.input[..input_samples],
                &mut self.output[..samples],
                frames,
                self.input_channels as u32,
                channels,
                &self.events,
                &self.parameter_events,
            )
            .is_ok()
    }

    fn run_begin(&mut self, frames: u32, channels: u32) -> Option<u32> {
        let samples = frames as usize * channels as usize;
        self.output[..samples].fill(0.0);
        if self.process_faulted {
            return Some(0);
        }
        let input_samples = frames as usize * self.input_channels;
        let parallel = self.parallel.as_mut()?;
        parallel
            .0
            .begin(
                &mut self.instance.0,
                &self.input[..input_samples],
                frames,
                &self.events,
                &self.parameter_events,
            )
            .ok()
    }

    fn unit_job(&mut self, unit: u32, frames: u32, channels: u32) -> UnitJob {
        let input_samples = frames as usize * self.input_channels;
        let parallel = self
            .parallel
            .as_mut()
            .expect("unit job requested for a classic desktop voice");
        // The borrow checker cannot see the field split through the Option;
        // the input slice is reborrowed from a raw pointer instead.
        let input_ptr = self.input.as_ptr();
        // SAFETY: `input` and `parallel` are disjoint fields, and the slice
        // outlives the block by the pool's contract.
        let input = unsafe { std::slice::from_raw_parts(input_ptr, input_samples) };
        parallel.0.unit_job(unit, input, frames, channels)
    }

    fn run_end(&mut self, frames: u32, channels: u32, completed: u32) -> bool {
        if self.process_faulted {
            return true;
        }
        let samples = frames as usize * channels as usize;
        let Some(parallel) = self.parallel.as_mut() else {
            return false;
        };
        parallel
            .0
            .finish(
                &mut self.instance.0,
                &mut self.output[..samples],
                frames,
                channels,
                completed,
            )
            .is_ok()
    }

    fn quarantine(&mut self) {
        self.output.fill(0.0);
        self.process_faulted = true;
    }
}

struct SendablePluginInstance(PluginInstance<'static>);

// SAFETY: the instance is moved once into CPAL's output callback, and every
// subsequent plugin ABI call happens serially on that callback. RackForge does
// not share the handle with the UI instance or access it after the move.
unsafe impl Send for SendablePluginInstance {}

struct SendableParallelUnits(ParallelUnits<'static>);

// SAFETY: unit instances are portable wasm-v1 sandboxes created next to the
// coordinator; ownership moves once into the audio callback and each unit is
// entered by at most one pool worker per block under the epoch protocol.
unsafe impl Send for SendableParallelUnits {}

struct MasterGain {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl MasterGain {
    fn new(level: MasterLevel) -> Self {
        let gain = level.amplitude();
        Self {
            current: gain,
            target: gain,
            step: 0.0,
            remaining: 0,
        }
    }

    fn set_level(&mut self, level: MasterLevel) {
        self.target = level.amplitude();
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

struct MasterBalance {
    current_left: f32,
    current_right: f32,
    target_left: f32,
    target_right: f32,
    step_left: f32,
    step_right: f32,
    remaining: u32,
}

impl MasterBalance {
    fn new(pan: MasterPan) -> Self {
        let (left, right) = pan.balance();
        Self {
            current_left: left,
            current_right: right,
            target_left: left,
            target_right: right,
            step_left: 0.0,
            step_right: 0.0,
            remaining: 0,
        }
    }

    fn set_pan(&mut self, pan: MasterPan) {
        (self.target_left, self.target_right) = pan.balance();
        self.remaining = MASTER_SMOOTHING_FRAMES;
        self.step_left = (self.target_left - self.current_left) / self.remaining as f32;
        self.step_right = (self.target_right - self.current_right) / self.remaining as f32;
    }

    fn next(&mut self) -> (f32, f32) {
        if self.remaining > 0 {
            self.current_left += self.step_left;
            self.current_right += self.step_right;
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current_left = self.target_left;
                self.current_right = self.target_right;
            }
        }
        (self.current_left, self.current_right)
    }
}

struct AudioProcessor {
    voices: Vec<AudioVoice>,
    active_voice: usize,
    midi_receiver: Receiver<MidiPacket>,
    command_receiver: Receiver<AudioCommand>,
    events: Vec<MidiEventV1>,
    parameter_events: Vec<ParameterEventV1>,
    parameter_links: Vec<CompiledParameterLink>,
    output: Vec<f32>,
    plugin_input: Vec<f32>,
    capture: Option<Arc<CaptureRing>>,
    capture_channels: usize,
    device_channels: usize,
    output_gain: f32,
    output_meter: Arc<OutputMeter>,
    master_gain: MasterGain,
    master_balance: MasterBalance,
    stopped: bool,
    /// True while LIVE is the surface on stage. Key-follow lanes listen only
    /// then: off the stage the same keyboard is an instrument being played.
    conducting: bool,
    retired_voice_sender: SyncSender<AudioVoice>,
    deferred_retire: Vec<AudioVoice>,
    live_parameter_writer: LiveParameterWriterHandle,
    sample_rate: u32,
    render_pool: RenderPool,
    render_telemetry: Arc<RenderTelemetry>,
    /// The host sequencer: transport and lanes, advanced once per block so
    /// pattern MIDI joins `events` sample-accurately before instances run.
    sequencer: rackforge_core::SequencerEngine,
    /// Realtime clock bytes leave the audio thread through here; a
    /// dedicated thread owns the MIDI output ports and writes them.
    clock_sender: SyncSender<u8>,
    clock_scratch: Vec<MidiEventV1>,
}

impl AudioProcessor {
    fn render(&mut self, frames: usize) -> Result<&[f32]> {
        if frames == 0 || frames > MAX_AUDIO_FRAMES {
            bail!("Windows requested an unsupported audio block of {frames} frames");
        }
        self.events.clear();
        self.parameter_events.clear();
        self.apply_commands()?;
        while self.events.len() < MAX_MIDI_EVENTS_PER_BLOCK {
            let Ok(packet) = self.midi_receiver.try_recv() else {
                break;
            };
            let ingress = packet.ingress();
            let active_instance_id = self.voices[self.active_voice].instance_id.as_str();
            let mut consume = false;
            for link in self
                .parameter_links
                .iter()
                .filter(|link| link.link.instance_id == active_instance_id)
            {
                let Some(mapped) = link.apply(ingress) else {
                    continue;
                };
                consume |=
                    mapped.pass_through == rackforge_midi_api::ParameterLinkPassThrough::Consume;
                if self.parameter_events.len() < MAX_MIDI_EVENTS_PER_BLOCK {
                    self.parameter_events.push(mapped.event);
                    self.live_parameter_writer.try_record(
                        self.voices[self.active_voice].live_parameter_target,
                        mapped.event.parameter_index,
                        mapped.event.value,
                    );
                }
            }
            if !consume {
                let conducted = self.conducting
                    && feed_sequencer_input(&mut self.sequencer, packet.data, packet.length);
                if !conducted {
                    push_midi_event(&mut self.events, packet);
                }
            }
        }
        // The sequencer advances whether or not anything is listening: the
        // transport is the machine's clock, not the instrument's. Its output
        // is appended after live input and the whole block is re-sorted by
        // frame inside render_block, offs before ons preserved.
        {
            let mut sequenced_params = std::mem::take(&mut self.parameter_events);
            let mut clock = std::mem::take(&mut self.clock_scratch);
            clock.clear();
            self.sequencer.render_block(
                frames as u32,
                &mut self.events,
                &mut sequenced_params,
                &mut clock,
            );
            // Sub-block clock timing is bounded by the buffer size; the
            // writer thread sends the bytes the moment they arrive. A full
            // queue drops pulses rather than stalling the callback — a
            // slave that missed one pulse free-wheels past it.
            for event in &clock {
                let _ = self.clock_sender.try_send(event.data[0]);
            }
            self.clock_scratch = clock;
            self.parameter_events = sequenced_params;
        }
        let samples = frames * PLUGIN_OUTPUT_CHANNELS;
        self.output[..samples].fill(0.0);
        if self.stopped {
            self.discard_capture(frames);
            return Ok(&self.output[..samples]);
        }
        let input_channels = self.voices[self.active_voice].input_channels;
        let output_channels = self.voices[self.active_voice].output_channels;
        self.prepare_plugin_input(frames, input_channels);
        let deadline_ns = frames as u64 * 1_000_000_000 / u64::from(self.sample_rate.max(1));
        let voice = &mut self.voices[self.active_voice];
        voice.input[..frames * input_channels]
            .copy_from_slice(&self.plugin_input[..frames * input_channels]);
        voice.events.clear();
        voice.events.extend_from_slice(&self.events);
        voice.parameter_events.clear();
        voice
            .parameter_events
            .extend_from_slice(&self.parameter_events);
        let was_faulted = voice.process_faulted;
        let render_started = Instant::now();
        let scheduled = self.render_pool.process(
            std::slice::from_mut(voice),
            frames as u32,
            output_channels as u32,
            deadline_ns,
        );
        if !scheduled {
            process_slots_sequential(
                std::slice::from_mut(voice),
                frames as u32,
                output_channels as u32,
                &self.render_telemetry,
            );
            self.render_telemetry.record_block(
                render_started.elapsed().as_nanos() as u64,
                deadline_ns,
                None,
            );
        }
        if voice.process_faulted && !was_faulted {
            eprintln!(
                "PLUGIN_PROCESS_QUARANTINED context=desktop:{} action=silence",
                voice.instance_id
            );
        }
        let plugin_output = &voice.output[..frames * output_channels];
        let output = &mut self.output[..samples];
        for frame in 0..frames {
            output[frame * 2] = plugin_output[frame * output_channels];
            output[frame * 2 + 1] = if output_channels > 1 {
                plugin_output[frame * output_channels + 1]
            } else {
                plugin_output[frame * output_channels]
            };
        }
        for frame in output.as_chunks_mut::<PLUGIN_OUTPUT_CHANNELS>().0 {
            let gain = self.master_gain.next();
            let (left, right) = self.master_balance.next();
            frame[0] *= gain * left;
            frame[1] *= gain * right;
        }
        Ok(output)
    }

    fn discard_capture(&self, frames: usize) {
        let Some(capture) = &self.capture else { return };
        for _ in 0..frames.saturating_mul(self.capture_channels) {
            let _ = capture.pop();
        }
    }

    fn prepare_plugin_input(&mut self, frames: usize, plugin_channels: usize) {
        if plugin_channels == 0 {
            self.discard_capture(frames);
            return;
        }
        let destination = &mut self.plugin_input[..frames * plugin_channels];
        destination.fill(0.0);
        let Some(capture) = &self.capture else { return };
        for frame in 0..frames {
            let mut captured = [0.0_f32; MAX_STANDALONE_INPUT_CHANNELS];
            for channel in 0..self.capture_channels {
                let sample = capture.pop();
                if channel < captured.len() {
                    captured[channel] = sample;
                }
            }
            for channel in 0..plugin_channels {
                destination[frame * plugin_channels + channel] = if self.capture_channels == 1 {
                    captured[0]
                } else if plugin_channels == 1 {
                    (captured[0] + captured[1]) * 0.5
                } else {
                    captured.get(channel).copied().unwrap_or(0.0)
                };
            }
        }
    }

    fn apply_commands(&mut self) -> Result<()> {
        self.flush_retired_voices();
        while let Ok(command) = self.command_receiver.try_recv() {
            match command {
                AudioCommand::SelectPlugin(instance_id) => {
                    let index = self
                        .voices
                        .iter()
                        .position(|voice| voice.instance_id == instance_id)
                        .with_context(|| format!("unknown audio plugin instance {instance_id}"))?;
                    if index != self.active_voice {
                        self.voices[self.active_voice]
                            .mirror_control(|instance| instance.reset())?;
                        self.active_voice = index;
                    }
                }
                AudioCommand::SaveActiveState { reply } => {
                    let result = self.voices[self.active_voice]
                        .instance
                        .0
                        .save_state()
                        .map_err(|error| error.to_string());
                    let _ = reply.try_send(result);
                }
                AudioCommand::RestoreState {
                    instance_id,
                    state,
                    reply,
                } => {
                    let result = (|| -> std::result::Result<(), String> {
                        let index = self
                            .voices
                            .iter()
                            .position(|voice| voice.instance_id == instance_id)
                            .ok_or_else(|| {
                                format!("unknown audio plugin instance {instance_id}")
                            })?;
                        self.voices[index]
                            .mirror_control(|instance| instance.load_state(&state))
                            .map_err(|error| error.to_string())?;
                        self.voices[index].process_faulted = false;
                        self.live_parameter_writer
                            .clear(self.voices[index].live_parameter_target);
                        if index != self.active_voice {
                            self.voices[self.active_voice]
                                .mirror_control(|instance| instance.reset())
                                .map_err(|error| error.to_string())?;
                            self.active_voice = index;
                        }
                        Ok(())
                    })();
                    let _ = reply.try_send(result);
                }
                AudioCommand::SelectSound {
                    instance_id,
                    sound_id,
                } => {
                    let index = self
                        .voices
                        .iter()
                        .position(|voice| voice.instance_id == instance_id)
                        .with_context(|| format!("unknown audio plugin instance {instance_id}"))?;
                    self.voices[index]
                        .mirror_control(|instance| instance.load_preset(&sound_id))?;
                    self.voices[index].process_faulted = false;
                    self.live_parameter_writer
                        .clear(self.voices[index].live_parameter_target);
                    if index != self.active_voice {
                        self.voices[self.active_voice]
                            .mirror_control(|instance| instance.reset())?;
                        self.active_voice = index;
                    }
                }
                AudioCommand::ActivateSurface {
                    instance_id,
                    request,
                    reply,
                } => {
                    let result = self
                        .voices
                        .iter_mut()
                        .find(|voice| voice.instance_id == instance_id)
                        .ok_or_else(|| format!("unknown audio plugin instance {instance_id}"))
                        .and_then(|voice| {
                            voice
                                .instance
                                .0
                                .activate_surface(&request)
                                .map_err(|error| error.to_string())
                        });
                    let _ = reply.try_send(result);
                }
                AudioCommand::PreviewProgram {
                    instance_id,
                    prepared,
                    reset,
                    reply,
                } => {
                    let result = (|| -> Result<(), String> {
                        let index = self
                            .voices
                            .iter()
                            .position(|voice| voice.instance_id == instance_id)
                            .ok_or_else(|| {
                                format!("unknown audio plugin instance {instance_id}")
                            })?;
                        if reset {
                            self.voices[index]
                                .mirror_control(|instance| instance.reset())
                                .map_err(|error| error.to_string())?;
                        }
                        let previewed = self.voices[index]
                            .instance
                            .0
                            .preview_program(&prepared)
                            .map_err(|error| error.to_string())?;
                        if previewed {
                            if let Some(parallel) = self.voices[index].parallel.as_mut() {
                                parallel
                                    .0
                                    .mirror(|instance| {
                                        instance.preview_program(&prepared).map(|_| ())
                                    })
                                    .map_err(|error| error.to_string())?;
                            }
                        } else {
                            self.voices[index]
                                .mirror_control(|instance| {
                                    instance.load_preset(&prepared.preview_sound_id)
                                })
                                .map_err(|error| error.to_string())?;
                        }
                        self.voices[index].process_faulted = false;
                        if index != self.active_voice {
                            self.voices[self.active_voice]
                                .mirror_control(|instance| instance.reset())
                                .map_err(|error| error.to_string())?;
                            self.active_voice = index;
                        }
                        self.stopped = false;
                        Ok(())
                    })();
                    let _ = reply.try_send(result);
                }
                AudioCommand::InstallProgram {
                    instance_id,
                    prepared,
                    reply,
                } => {
                    let result = (|| -> Result<PresetCatalog, String> {
                        let voice = self
                            .voices
                            .iter_mut()
                            .find(|voice| voice.instance_id == instance_id)
                            .ok_or_else(|| {
                                format!("unknown audio plugin instance {instance_id}")
                            })?;
                        voice
                            .mirror_control(|instance| instance.install_program(&prepared))
                            .map_err(|error| error.to_string())?;
                        voice
                            .instance
                            .0
                            .preset_catalog()
                            .map_err(|error| error.to_string())
                    })();
                    let _ = reply.try_send(result);
                }
                AudioCommand::RestoreProgram {
                    instance_id,
                    sound_id,
                    reply,
                } => {
                    let result = (|| -> Result<(), String> {
                        let index = self
                            .voices
                            .iter()
                            .position(|voice| voice.instance_id == instance_id)
                            .ok_or_else(|| {
                                format!("unknown audio plugin instance {instance_id}")
                            })?;
                        self.voices[index]
                            .mirror_control(|instance| instance.reset())
                            .map_err(|error| error.to_string())?;
                        if let Some(sound_id) = sound_id {
                            self.voices[index]
                                .mirror_control(|instance| instance.load_preset(&sound_id))
                                .map_err(|error| error.to_string())?;
                        }
                        self.voices[index].process_faulted = false;
                        if index != self.active_voice {
                            self.voices[self.active_voice]
                                .mirror_control(|instance| instance.reset())
                                .map_err(|error| error.to_string())?;
                            self.active_voice = index;
                        }
                        Ok(())
                    })();
                    let _ = reply.try_send(result);
                }
                AudioCommand::PluginParameters { instance_id, reply } => {
                    let result = self
                        .voices
                        .iter_mut()
                        .find(|voice| voice.instance_id == instance_id)
                        .ok_or_else(|| format!("unknown audio plugin instance {instance_id}"))
                        .and_then(|voice| {
                            let schema = voice.parameters.clone();
                            let values = schema
                                .parameters
                                .iter()
                                .map(|parameter| {
                                    voice
                                        .instance
                                        .0
                                        .get_parameter(parameter.index)
                                        .map(|value| PluginParameterValue {
                                            index: parameter.index,
                                            value,
                                        })
                                        .map_err(|error| error.to_string())
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            Ok((schema, values))
                        });
                    let _ = reply.try_send(result);
                }
                AudioCommand::SetPluginParameter {
                    instance_id,
                    parameter_index,
                    value,
                    reply,
                } => {
                    let result = self
                        .voices
                        .iter_mut()
                        .find(|voice| voice.instance_id == instance_id)
                        .ok_or_else(|| format!("unknown audio plugin instance {instance_id}"))
                        .and_then(|voice| {
                            let parameter = voice
                                .parameters
                                .parameters
                                .iter()
                                .find(|parameter| parameter.index == parameter_index)
                                .ok_or_else(|| {
                                    format!("unknown plugin parameter {parameter_index}")
                                })?;
                            if parameter.flags.read_only
                                || matches!(parameter.kind, ParameterKind::Meter { .. })
                            {
                                return Err(format!(
                                    "plugin parameter {} is read-only",
                                    parameter.id
                                ));
                            }
                            if !parameter_value_is_valid(&parameter.kind, value) {
                                return Err(format!(
                                    "invalid value {value} for plugin parameter {}",
                                    parameter.id
                                ));
                            }
                            voice
                                .mirror_control(|instance| {
                                    instance.set_parameter(parameter_index, value)
                                })
                                .map_err(|error| error.to_string())?;
                            let canonical = voice
                                .instance
                                .0
                                .get_parameter(parameter_index)
                                .map_err(|error| error.to_string())?;
                            self.live_parameter_writer.try_record(
                                voice.live_parameter_target,
                                parameter_index,
                                canonical,
                            );
                            Ok(canonical)
                        });
                    let _ = reply.try_send(result);
                }
                AudioCommand::ReplaceParameterLinks { links, reply } => {
                    self.parameter_links = links;
                    let _ = reply.try_send(Ok(()));
                }
                AudioCommand::InjectMidi(packet) => {
                    let conducted = self.conducting
                        && feed_sequencer_input(&mut self.sequencer, packet.data, packet.length);
                    if !conducted {
                        push_midi_event(&mut self.events, packet);
                    }
                }
                AudioCommand::Sequencer { command, reply } => {
                    let _ = reply.try_send(self.sequencer.apply(&command));
                }
                AudioCommand::SequencerStatus { reply } => {
                    let _ = reply.try_send(self.sequencer.status());
                }
                AudioCommand::SequencerCaptureTake { lane, reply } => {
                    let _ = reply.try_send(self.sequencer.capture_take(lane));
                }
                AudioCommand::SetMasterLevel(level) => self.master_gain.set_level(level),
                AudioCommand::SetMasterPan(pan) => self.master_balance.set_pan(pan),
                AudioCommand::SetRunning(running) => self.stopped = !running,
                AudioCommand::SetConducting(conducting) => self.conducting = conducting,
                AudioCommand::EmergencyStop => {
                    for voice in &mut self.voices {
                        voice.mirror_control(|instance| instance.reset())?;
                        voice.process_faulted = false;
                    }
                    self.stopped = true;
                }
                AudioCommand::ReplaceVoice(voice) => {
                    let index = self
                        .voices
                        .iter()
                        .position(|current| current.instance_id == voice.instance_id)
                        .with_context(|| {
                            format!("unknown audio plugin instance {}", voice.instance_id)
                        })?;
                    let retired = std::mem::replace(&mut self.voices[index], voice);
                    self.deferred_retire.push(retired);
                }
            }
        }
        Ok(())
    }

    fn flush_retired_voices(&mut self) {
        while let Some(voice) = self.deferred_retire.pop() {
            match self.retired_voice_sender.try_send(voice) {
                Ok(()) => {}
                Err(TrySendError::Full(voice)) => {
                    self.deferred_retire.push(voice);
                    break;
                }
                Err(TrySendError::Disconnected(voice)) => {
                    self.deferred_retire.push(voice);
                    break;
                }
            }
        }
    }
}

fn prepare_audio_voice(
    spec: VoiceSpec,
    sample_rate: u32,
    live_parameter_target: usize,
    live_parameter_store: &LiveParameterStateStore,
) -> Result<AudioVoice> {
    let audio = spec.plugin.manifest().resolved_audio_contract();
    let input_channels = audio.input_channels() as usize;
    let output_channels = audio.output_channels() as usize;
    if input_channels > MAX_STANDALONE_INPUT_CHANNELS {
        bail!(
            "Desktop PLAY supports at most {MAX_STANDALONE_INPUT_CHANNELS} plugin input channels, {} declares {input_channels}",
            spec.instance_id
        );
    }
    if !(1..=PLUGIN_OUTPUT_CHANNELS).contains(&output_channels) {
        bail!(
            "Desktop PLAY requires one or two plugin output channels, {} declares {output_channels}",
            spec.instance_id
        );
    }
    let parameters = spec.plugin.parameters().clone();
    let mut instance = spec
        .plugin
        .create_instance_with_resource_overrides(&spec.resources)
        .with_context(|| format!("creating audio instance {}", spec.instance_id))?;
    if let Some(preset_id) = spec.preset_id.as_deref() {
        instance
            .load_preset(preset_id)
            .with_context(|| format!("loading preset {preset_id:?} for the audio engine"))?;
    }
    if let Some(state) = spec.initial_state.as_deref() {
        // Best effort: a state from an incompatible build is simply skipped
        // and the plugin keeps its defaults.
        if let Err(error) = instance.load_state(state) {
            eprintln!(
                "DESKTOP_LIVE_STATE_SKIPPED instance={} error={error:#}",
                spec.instance_id
            );
        }
    }
    let restored_parameters: Vec<(u32, f64)> =
        live_parameter_store.restored_values(&spec.plugin.manifest().id, spec.plugin.parameters());
    for (parameter_index, value) in restored_parameters.iter().copied() {
        rackforge_core::set_plugin_parameter(spec.plugin, &mut instance, parameter_index, value)
            .with_context(|| {
                format!(
                    "restoring live parameter {parameter_index} for {}",
                    spec.instance_id
                )
            })?;
    }
    instance
        .activate(
            f64::from(sample_rate),
            MAX_AUDIO_FRAMES as u32,
            input_channels as u32,
            output_channels as u32,
        )
        .with_context(|| format!("activating audio instance {}", spec.instance_id))?;
    // PLAY unit instances mirror the same canonical inputs the coordinator
    // just received: program, state and restored parameters. This runs on a
    // control/setup thread, never inside the audio callback.
    let mut parallel = if rackforge_core::parallel_render::parallel_units_enabled() {
        ParallelUnits::create_with_resources(
            spec.plugin,
            &spec.resources,
            f64::from(sample_rate),
            MAX_AUDIO_FRAMES as u32,
            input_channels as u32,
            output_channels as u32,
        )
        .with_context(|| format!("preparing PLAY units for {}", spec.instance_id))?
    } else {
        None
    };
    if let Some(units) = parallel.as_mut() {
        if let Some(preset_id) = spec.preset_id.as_deref() {
            units
                .mirror(|instance| instance.load_preset(preset_id))
                .with_context(|| format!("mirroring preset for {}", spec.instance_id))?;
        }
        if let Some(state) = spec.initial_state.as_deref() {
            // Mirrors the coordinator's best-effort restore: a state the
            // coordinator skipped is skipped here too.
            let _ = units.mirror(|instance| instance.load_state(state));
        }
        for (parameter_index, value) in restored_parameters.iter().copied() {
            units
                .mirror(|instance| instance.set_parameter(parameter_index, value))
                .with_context(|| {
                    format!(
                        "mirroring live parameter {parameter_index} for {}",
                        spec.instance_id
                    )
                })?;
        }
    }
    Ok(AudioVoice {
        instance_id: spec.instance_id,
        parameters,
        input_channels,
        output_channels,
        instance: SendablePluginInstance(instance),
        parallel: parallel.map(SendableParallelUnits),
        live_parameter_target,
        input: vec![0.0; MAX_AUDIO_FRAMES * input_channels.max(1)],
        output: vec![0.0; MAX_AUDIO_FRAMES * PLUGIN_OUTPUT_CHANNELS],
        events: Vec::with_capacity(MAX_MIDI_EVENTS_PER_BLOCK),
        parameter_events: Vec::with_capacity(MAX_MIDI_EVENTS_PER_BLOCK),
        process_faulted: false,
    })
}

/// The clock writer: owns every MIDI output port and forwards realtime
/// bytes the moment the audio thread emits them. Ports are enumerated on
/// first use and re-enumerated whenever a write fails, so replugging a
/// box mid-set recovers on the next pulse.
fn spawn_clock_writer(receiver: Receiver<u8>) {
    let _ = thread::Builder::new()
        .name("rackforge-midi-clock".into())
        .spawn(move || {
            let mut connections: Vec<midir::MidiOutputConnection> = Vec::new();
            let mut connected = false;
            while let Ok(byte) = receiver.recv() {
                if !connected {
                    connections = open_clock_outputs();
                    connected = true;
                }
                let mut failed = false;
                for connection in &mut connections {
                    if connection.send(&[byte]).is_err() {
                        failed = true;
                    }
                }
                if failed || connections.is_empty() {
                    connected = false;
                }
            }
        });
}

fn open_clock_outputs() -> Vec<midir::MidiOutputConnection> {
    let mut connections = Vec::new();
    let Ok(probe) = midir::MidiOutput::new("rackforge clock probe") else {
        return connections;
    };
    let count = probe.ports().len();
    let _ = probe;
    for index in 0..count {
        let Ok(output) = midir::MidiOutput::new("rackforge MIDI clock") else {
            continue;
        };
        let ports = output.ports();
        let Some(port) = ports.get(index) else {
            continue;
        };
        if let Ok(connection) = output.connect(port, "rackforge clock") {
            connections.push(connection);
        }
    }
    connections
}

/// Key-follow lanes listen to the player's keyboard: every live note that
/// reaches the instrument also reaches the engine's gate. Answers true
/// when a FOLLOW lane claimed the note as a conducting gesture — the
/// caller must not let it reach the instrument.
fn feed_sequencer_input(
    sequencer: &mut rackforge_core::SequencerEngine,
    data: [u8; 3],
    length: u8,
) -> bool {
    if length < 3 {
        return false;
    }
    let channel = data[0] & 0x0f;
    match data[0] & 0xf0 {
        0x90 if data[2] > 0 => sequencer.note_input(channel, data[1], data[2], true),
        0x80 | 0x90 => sequencer.note_input(channel, data[1], 0, false),
        _ => false,
    }
}

fn push_midi_event(events: &mut Vec<MidiEventV1>, packet: MidiPacket) {
    if events.len() < MAX_MIDI_EVENTS_PER_BLOCK {
        events.push(MidiEventV1 {
            frame: 0,
            length: packet.length,
            data: packet.data,
        });
    }
}

fn capture_input(
    data: &cpal::Data,
    format: SampleFormat,
    device_channels: usize,
    selected_channels: &[usize],
    input_gain: f32,
    ring: &CaptureRing,
) -> Result<()> {
    match format {
        SampleFormat::I8 => {
            capture_samples::<i8>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::I16 => {
            capture_samples::<i16>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::I24 => {
            capture_samples::<cpal::I24>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::I32 => {
            capture_samples::<i32>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::I64 => {
            capture_samples::<i64>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::U8 => {
            capture_samples::<u8>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::U16 => {
            capture_samples::<u16>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::U32 => {
            capture_samples::<u32>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::U64 => {
            capture_samples::<u64>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::F32 => {
            capture_samples::<f32>(data, device_channels, selected_channels, input_gain, ring)
        }
        SampleFormat::F64 => {
            capture_samples::<f64>(data, device_channels, selected_channels, input_gain, ring)
        }
        _ => bail!("unsupported Windows input sample format {format:?}"),
    }
}

fn capture_samples<T>(
    data: &cpal::Data,
    device_channels: usize,
    selected_channels: &[usize],
    input_gain: f32,
    ring: &CaptureRing,
) -> Result<()>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let input = data
        .as_slice::<T>()
        .context("Windows returned an audio input buffer with the wrong sample type")?;
    if device_channels == 0 || input.len() % device_channels != 0 {
        bail!("Windows audio input buffer changed channel layout during capture");
    }
    for frame in input.chunks_exact(device_channels) {
        let mut selected = [0.0_f32; MAX_STANDALONE_INPUT_CHANNELS];
        for (target, &channel) in selected_channels.iter().enumerate() {
            let sample = frame.get(channel).copied().unwrap_or(T::EQUILIBRIUM);
            selected[target] = clean_sample(f32::from_sample(sample) * input_gain);
        }
        ring.push_frame(&selected[..selected_channels.len()]);
    }
    Ok(())
}

fn render_output(
    processor: &mut AudioProcessor,
    data: &mut cpal::Data,
    format: SampleFormat,
) -> Result<()> {
    let frames = data.len() / processor.device_channels;
    let device_channels = processor.device_channels;
    let output_gain = processor.output_gain;
    let output_meter = Arc::clone(&processor.output_meter);
    let rendered = processor.render(frames)?;
    for frame in rendered.as_chunks::<PLUGIN_OUTPUT_CHANNELS>().0 {
        output_meter.observe_stereo(frame[0] * output_gain, frame[1] * output_gain);
    }
    match format {
        SampleFormat::I8 => copy_samples::<i8>(data, rendered, device_channels, output_gain),
        SampleFormat::I16 => copy_samples::<i16>(data, rendered, device_channels, output_gain),
        SampleFormat::I24 => {
            copy_samples::<cpal::I24>(data, rendered, device_channels, output_gain)
        }
        SampleFormat::I32 => copy_samples::<i32>(data, rendered, device_channels, output_gain),
        SampleFormat::I64 => copy_samples::<i64>(data, rendered, device_channels, output_gain),
        SampleFormat::U8 => copy_samples::<u8>(data, rendered, device_channels, output_gain),
        SampleFormat::U16 => copy_samples::<u16>(data, rendered, device_channels, output_gain),
        SampleFormat::U32 => copy_samples::<u32>(data, rendered, device_channels, output_gain),
        SampleFormat::U64 => copy_samples::<u64>(data, rendered, device_channels, output_gain),
        SampleFormat::F32 => copy_samples::<f32>(data, rendered, device_channels, output_gain),
        SampleFormat::F64 => copy_samples::<f64>(data, rendered, device_channels, output_gain),
        _ => bail!("unsupported Windows sample format {format:?}"),
    }
}

fn copy_samples<T>(
    data: &mut cpal::Data,
    rendered: &[f32],
    device_channels: usize,
    output_gain: f32,
) -> Result<()>
where
    T: SizedSample + FromSample<f32>,
{
    let output = data
        .as_slice_mut::<T>()
        .context("Windows returned an audio buffer with the wrong sample type")?;
    write_samples(output, rendered, device_channels, output_gain)
}

fn write_samples<T>(
    output: &mut [T],
    rendered: &[f32],
    device_channels: usize,
    output_gain: f32,
) -> Result<()>
where
    T: SizedSample + FromSample<f32>,
{
    let frames = rendered.len() / PLUGIN_OUTPUT_CHANNELS;
    if output.len() != frames * device_channels {
        bail!("Windows audio buffer changed length during rendering");
    }
    for frame in 0..frames {
        let left = clean_sample(rendered[frame * 2] * output_gain);
        let right = clean_sample(rendered[frame * 2 + 1] * output_gain);
        if device_channels == 1 {
            output[frame] = T::from_sample((left + right) * 0.5);
        } else {
            let base = frame * device_channels;
            output[base] = T::from_sample(left);
            output[base + 1] = T::from_sample(right);
            for sample in &mut output[base + 2..base + device_channels] {
                *sample = T::from_sample(0.0);
            }
        }
    }
    Ok(())
}

fn default_output_gain_db() -> i8 {
    DEFAULT_OUTPUT_GAIN_DB
}

fn default_input_gain_db() -> i8 {
    DEFAULT_INPUT_GAIN_DB
}

fn db_to_amplitude(db: i8) -> f32 {
    10.0_f32.powf(f32::from(db) / 20.0)
}

fn clean_sample(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

fn silence_output(data: &mut cpal::Data, format: SampleFormat) {
    macro_rules! fill {
        ($type:ty) => {
            if let Some(output) = data.as_slice_mut::<$type>() {
                output.fill(<$type>::from_sample(0.0));
            }
        };
    }
    match format {
        SampleFormat::I8 => fill!(i8),
        SampleFormat::I16 => fill!(i16),
        SampleFormat::I24 => fill!(cpal::I24),
        SampleFormat::I32 => fill!(i32),
        SampleFormat::I64 => fill!(i64),
        SampleFormat::U8 => fill!(u8),
        SampleFormat::U16 => fill!(u16),
        SampleFormat::U32 => fill!(u32),
        SampleFormat::U64 => fill!(u64),
        SampleFormat::F32 => fill!(f32),
        SampleFormat::F64 => fill!(f64),
        _ => data.bytes_mut().fill(0),
    }
}

fn discover_midi_inputs() -> Result<Vec<String>> {
    let discovery =
        MidiInput::new("rackforge-desktop-discovery").context("starting Windows MIDI discovery")?;
    let mut names = discovery
        .ports()
        .iter()
        .filter_map(|port| discovery.port_name(port).ok())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    Ok(names)
}

pub fn midi_source_descriptor(name: &str) -> Result<MidiSourceDescriptor> {
    Ok(MidiSourceDescriptor {
        id: stable_midi_source_id(name)?,
        name: name.to_owned(),
        primary: false,
    })
}

pub fn stable_midi_source_id(name: &str) -> Result<MidiSourceId> {
    let digest = Sha256::digest(name.trim().to_lowercase().as_bytes());
    MidiSourceId::new(format!("windows.midir.{:x}", digest)).map_err(|error| anyhow::anyhow!(error))
}

pub fn stable_midi_source_key(name: &str) -> MidiSourceKey {
    let id = stable_midi_source_id(name).expect("Windows MIDI names produce valid source ids");
    stable_midi_source_key_from_id(&id)
}

pub fn stable_midi_source_key_from_id(id: &MidiSourceId) -> MidiSourceKey {
    let digest = Sha256::digest(id.as_str().as_bytes());
    MidiSourceKey::new(u32::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3],
    ]))
}

struct MidiSupervisor {
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl MidiSupervisor {
    fn start(
        sender: SyncSender<MidiPacket>,
        selected: Vec<String>,
        telemetry: Arc<AudioTelemetry>,
        controller_sender: SyncSender<DesktopControllerEvent>,
        display_mailbox: ScreenMailbox,
        yield_keylab: bool,
    ) -> Result<(Self, Vec<String>)> {
        // When an installed controller package is enabled, ITS driver owns
        // the surface: the built-in KeyLab handling stands down and the
        // surface endpoint (Windows MIDI ports are exclusive-open) is left
        // for the driver. Note endpoints (ALV and friends) stay captured.
        let selected = selected
            .into_iter()
            .filter(|name| !(yield_keylab && keylab_controller::little_driver(name).is_some()))
            .collect::<BTreeSet<_>>();
        let (stop_sender, stop_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("rackforge-desktop-midi-supervisor".into())
            .spawn(move || {
                let mut connections = BTreeMap::new();
                let mut display = None;
                match reconcile_midi_inputs(
                    &selected,
                    &mut connections,
                    &sender,
                    &telemetry,
                    &controller_sender,
                    yield_keylab,
                ) {
                    Ok(names) => {
                        if !yield_keylab && reconcile_keylab_display(&mut display, &display_mailbox)
                        {
                            reconnect_keylab_inputs(
                                &selected,
                                &mut connections,
                                &sender,
                                &telemetry,
                                &controller_sender,
                            );
                        }
                        let _ = ready_sender.send(Ok(names));
                    }
                    Err(error) => {
                        let _ = ready_sender.send(Err(format!("{error:#}")));
                        return;
                    }
                }
                let mut next_reconcile = Instant::now() + MIDI_RECONNECT_INTERVAL;
                loop {
                    match stop_receiver.recv_timeout(MIDI_SUPERVISOR_TICK) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {}
                    }
                    // The mailbox owns coalescing: while SysEx is settling,
                    // producers replace the pending semantic screen and the
                    // next tick takes only the newest revision.
                    if let Some(display) = display.as_mut()
                        && let Some(update) = display_mailbox.take()
                        && let Err(error) = display.render(&update)
                    {
                        eprintln!("DESKTOP_KEYLAB_DISPLAY_FAILED error={error:#}");
                        display_mailbox.invalidate_delivery();
                        display.restore_best_effort();
                    }
                    if display.as_ref().is_some_and(|display| display.failed) {
                        display = None;
                    }
                    if Instant::now() >= next_reconcile {
                        if let Err(error) = reconcile_midi_inputs(
                            &selected,
                            &mut connections,
                            &sender,
                            &telemetry,
                            &controller_sender,
                            yield_keylab,
                        ) {
                            eprintln!("DESKTOP_MIDI_SCAN_FAILED error={error:#}");
                        }
                        if !yield_keylab && reconcile_keylab_display(&mut display, &display_mailbox)
                        {
                            reconnect_keylab_inputs(
                                &selected,
                                &mut connections,
                                &sender,
                                &telemetry,
                                &controller_sender,
                            );
                        }
                        next_reconcile = Instant::now() + MIDI_RECONNECT_INTERVAL;
                    }
                }
                if let Some(mut display) = display {
                    display.restore_best_effort();
                }
            })
            .context("starting Windows MIDI hotplug supervisor")?;
        let connected = ready_receiver
            .recv()
            .context("Windows MIDI hotplug supervisor stopped during startup")?;
        match connected {
            Ok(names) => Ok((
                Self {
                    stop: stop_sender,
                    worker: Some(worker),
                },
                names,
            )),
            Err(error) => {
                let _ = stop_sender.send(());
                let _ = worker.join();
                bail!("starting Windows MIDI hotplug supervisor: {error}")
            }
        }
    }
}

impl Drop for MidiSupervisor {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn reconcile_midi_inputs(
    selected: &BTreeSet<String>,
    connections: &mut BTreeMap<String, MidiInputConnection<()>>,
    sender: &SyncSender<MidiPacket>,
    telemetry: &Arc<AudioTelemetry>,
    controller_sender: &SyncSender<DesktopControllerEvent>,
    yield_keylab: bool,
) -> Result<Vec<String>> {
    let present = discover_midi_inputs()?.into_iter().collect::<BTreeSet<_>>();
    let desired = desired_midi_inputs(selected, &present, yield_keylab);
    let lost = connections
        .keys()
        .filter(|name| !present.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    for name in lost {
        connections.remove(&name);
        eprintln!("DESKTOP_MIDI_SOURCE_LOST name={name:?}");
        if keylab_controller::little_driver(&name).is_some() {
            let _ = controller_sender.try_send(DesktopControllerEvent::Disconnected);
        }
        release_held_notes(sender, telemetry);
    }
    for name in &desired {
        if !present.contains(name) || connections.contains_key(name) {
            continue;
        }
        match connect_midi_input(
            name,
            sender.clone(),
            Arc::clone(telemetry),
            controller_sender.clone(),
        ) {
            Ok(connection) => {
                connections.insert(name.clone(), connection);
                println!("DESKTOP_MIDI_SOURCE_CONNECTED name={name:?}");
                if keylab_controller::little_driver(name).is_some() {
                    let _ = controller_sender.try_send(DesktopControllerEvent::Connected);
                }
            }
            Err(error) => {
                eprintln!("DESKTOP_MIDI_CONNECT_FAILED name={name:?} error={error:#}");
            }
        }
    }
    Ok(connections.keys().cloned().collect())
}

fn desired_midi_inputs(
    selected: &BTreeSet<String>,
    present: &BTreeSet<String>,
    yield_keylab: bool,
) -> BTreeSet<String> {
    let mut desired = selected.clone();
    // The built-in surface handling force-captures the KeyLab's surface
    // port -- unless an installed controller package owns the hardware,
    // in which case that port belongs to ITS driver.
    if !yield_keylab {
        desired.extend(
            present
                .iter()
                .filter(|name| keylab_controller::little_driver(name).is_some())
                .cloned(),
        );
    }
    desired
}

fn connect_midi_input(
    name: &str,
    sender: SyncSender<MidiPacket>,
    telemetry: Arc<AudioTelemetry>,
    controller_sender: SyncSender<DesktopControllerEvent>,
) -> Result<MidiInputConnection<()>> {
    let mut midi =
        MidiInput::new("rackforge-desktop-midi").context("opening a Windows MIDI client")?;
    midi.ignore(Ignore::None);
    let port = midi
        .ports()
        .into_iter()
        .find(|port| midi.port_name(port).as_deref() == Ok(name))
        .with_context(|| format!("Windows MIDI input {name:?} disappeared before connection"))?;
    let keylab = keylab_controller::is_keylab_endpoint(name);
    let source = stable_midi_source_key(name);
    midi.connect(
        &port,
        "rackforge-desktop-input",
        move |_timestamp, message, _| {
            if !message.is_empty() && message.len() <= 3 {
                let mut data = [0; 3];
                data[..message.len()].copy_from_slice(message);
                let _ = controller_sender.try_send(DesktopControllerEvent::MidiObserved {
                    source,
                    length: message.len() as u8,
                    data,
                    observed_at: Instant::now(),
                });
            }
            if keylab && let Some(event) = keylab_protocol::parse_input(message) {
                let event = match event {
                    keylab_protocol::ControllerEvent::Surface { input, phase } => {
                        DesktopControllerEvent::Surface { input, phase }
                    }
                };
                let _ = controller_sender.try_send(event);
                return;
            }
            if keylab
                && let Some(input) = keylab_controller::package_profile()
                    .semantic_profile
                    .as_ref()
                    .and_then(|profile| rackforge_parameter_input(profile, message))
            {
                let _ =
                    controller_sender.try_send(DesktopControllerEvent::RackForgeParameter(input));
                return;
            }
            if keylab
                && let Some(input) = keylab_controller::package_profile()
                    .semantic_profile
                    .as_ref()
                    .and_then(|profile| semantic_control_input(profile, message))
            {
                let _ = controller_sender.try_send(DesktopControllerEvent::SemanticControl(input));
            }
            if message.is_empty() || message.len() > 3 {
                return;
            }
            let mut data = [0; 3];
            data[..message.len()].copy_from_slice(message);
            if sender
                .try_send(MidiPacket {
                    source,
                    length: message.len() as u8,
                    data,
                    wide: None,
                })
                .is_err()
            {
                telemetry
                    .midi_dropped_events
                    .fetch_add(1, Ordering::Relaxed);
            }
        },
        (),
    )
    .map_err(|error| anyhow::anyhow!("connecting Windows MIDI input {name:?}: {error}"))
}

struct KeyLabDisplay {
    connection: MidiOutputConnection,
    switched_to_daw: bool,
    connected: bool,
    failed: bool,
}

impl KeyLabDisplay {
    fn open(port_name: &str) -> Result<Self> {
        let midi = MidiOutput::new("rackforge-desktop-keylab")
            .context("opening the Arturia display MIDI client")?;
        let port = midi
            .ports()
            .into_iter()
            .find(|port| midi.port_name(port).as_deref() == Ok(port_name))
            .with_context(|| format!("Arturia display output {port_name:?} disappeared"))?;
        let connection = midi
            .connect(&port, "rackforge-desktop-keylab-display")
            .map_err(|error| {
                anyhow::anyhow!("connecting Arturia display {port_name:?}: {error}")
            })?;
        let mut display = Self {
            connection,
            switched_to_daw: false,
            connected: false,
            failed: false,
        };
        display.switched_to_daw = true;
        display.connected = true;
        let acquire = keylab_protocol::acquire_messages().map_err(anyhow::Error::msg)?;
        if let Err(error) = display.send_messages(acquire) {
            display.restore_best_effort();
            return Err(error);
        }
        println!("DESKTOP_KEYLAB_LITTLE_ACQUIRED name={port_name:?}");
        Ok(display)
    }

    fn send(&mut self, message: &[u8]) -> Result<()> {
        self.connection
            .send(message)
            .map_err(|error| anyhow::anyhow!("sending Arturia SysEx: {error}"))
    }

    fn send_messages(
        &mut self,
        messages: impl IntoIterator<Item = keylab_protocol::OutboundMessage>,
    ) -> Result<()> {
        for message in messages {
            self.send(&message.bytes)?;
            if message.settle_after_ms != 0 {
                thread::sleep(Duration::from_millis(u64::from(message.settle_after_ms)));
            }
        }
        Ok(())
    }

    fn render(&mut self, update: &ScreenUpdate) -> Result<()> {
        let messages =
            keylab_protocol::render_update_messages(update).map_err(anyhow::Error::msg)?;
        if let Err(error) = self.send_messages(messages) {
            self.failed = true;
            return Err(error);
        }
        Ok(())
    }

    fn restore_best_effort(&mut self) {
        if self.connected || self.switched_to_daw {
            if let Ok(messages) = keylab_protocol::restore_messages() {
                let _ = self.send_messages(messages);
            }
            self.connected = false;
            self.switched_to_daw = false;
        }
    }
}

fn reconcile_keylab_display(
    display: &mut Option<KeyLabDisplay>,
    display_mailbox: &ScreenMailbox,
) -> bool {
    if display.is_some() {
        return false;
    }
    let outputs = match discover_midi_outputs() {
        Ok(outputs) => outputs.into_iter().collect::<BTreeSet<_>>(),
        Err(error) => {
            eprintln!("DESKTOP_MIDI_OUTPUT_SCAN_FAILED error={error:#}");
            return false;
        }
    };
    let Some(name) = outputs
        .iter()
        .find(|name| keylab_controller::little_driver(name).is_some())
    else {
        return false;
    };
    match KeyLabDisplay::open(name) {
        Ok(mut opened) => {
            // The hardware may have been unplugged or switched back to its
            // factory preset. Never trust an earlier delivery marker after
            // acquisition: the first update must be a complete snapshot.
            display_mailbox.invalidate_delivery();
            if let Some(update) = display_mailbox.take()
                && let Err(error) = opened.render(&update)
            {
                eprintln!("DESKTOP_KEYLAB_INITIAL_RENDER_FAILED error={error:#}");
                display_mailbox.invalidate_delivery();
                opened.restore_best_effort();
                return false;
            }
            *display = Some(opened);
            true
        }
        Err(error) => {
            eprintln!("DESKTOP_KEYLAB_ACQUIRE_FAILED name={name:?} error={error:#}");
            false
        }
    }
}

fn reconnect_keylab_inputs(
    selected: &BTreeSet<String>,
    connections: &mut BTreeMap<String, MidiInputConnection<()>>,
    sender: &SyncSender<MidiPacket>,
    telemetry: &Arc<AudioTelemetry>,
    controller_sender: &SyncSender<DesktopControllerEvent>,
) {
    let names = connections
        .keys()
        .filter(|name| keylab_controller::is_keylab_endpoint(name))
        .cloned()
        .collect::<Vec<_>>();
    for name in names {
        connections.remove(&name);
    }
    thread::sleep(Duration::from_millis(100));
    if let Err(error) = reconcile_midi_inputs(
        selected,
        connections,
        sender,
        telemetry,
        controller_sender,
        false,
    ) {
        eprintln!("DESKTOP_KEYLAB_INPUT_REOPEN_FAILED error={error:#}");
    } else {
        println!("DESKTOP_KEYLAB_INPUTS_REOPENED");
    }
}

fn discover_midi_outputs() -> Result<Vec<String>> {
    let discovery = MidiOutput::new("rackforge-desktop-output-discovery")
        .context("starting Windows MIDI output discovery")?;
    let mut names = discovery
        .ports()
        .iter()
        .filter_map(|port| discovery.port_name(port).ok())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    Ok(names)
}

fn release_held_notes(sender: &SyncSender<MidiPacket>, telemetry: &AudioTelemetry) {
    let packets = panic_packets(PanicScope::AllChannels);
    let count = packets.len();
    for packet in packets {
        if sender
            .send(MidiPacket {
                source: VIRTUAL_MIDI_SOURCE_KEY,
                length: packet.length,
                data: packet.data,
                wide: None,
            })
            .is_err()
        {
            return;
        }
    }
    telemetry.midi_panic_count.fetch_add(1, Ordering::Relaxed);
    println!("DESKTOP_MIDI_PANIC_SENT packets={count} scope=AllChannels");
}

fn supported_sample_rates(
    configs: &[cpal::SupportedStreamConfigRange],
    default_rate: u32,
) -> Vec<u32> {
    let mut rates = BTreeSet::from([default_rate]);
    for rate in COMMON_SAMPLE_RATES {
        if configs
            .iter()
            .any(|config| config.min_sample_rate().0 <= rate && rate <= config.max_sample_rate().0)
        {
            rates.insert(rate);
        }
    }
    rates.into_iter().collect()
}

fn supported_buffer_frames(configs: &[cpal::SupportedStreamConfigRange]) -> Vec<u32> {
    COMMON_BUFFER_FRAMES
        .into_iter()
        .filter(|frames| {
            configs
                .iter()
                .any(|config| buffer_supported(config.buffer_size(), Some(*frames)))
        })
        .collect()
}

fn buffer_supported(supported: &SupportedBufferSize, requested: Option<u32>) -> bool {
    let Some(requested) = requested else {
        return true;
    };
    requested <= MAX_AUDIO_FRAMES as u32
        && match supported {
            SupportedBufferSize::Range { min, max } => *min <= requested && requested <= *max,
            SupportedBufferSize::Unknown => true,
        }
}

fn publish_error(slot: &Mutex<Option<String>>, error: impl ToString) {
    if let Ok(mut slot) = slot.lock()
        && slot.is_none()
    {
        *slot = Some(error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_round_trip_as_toml() {
        let preferences = AudioPreferences {
            schema_version: AUDIO_SCHEMA_VERSION,
            driver: "WASAPI".into(),
            output_device: "Speakers".into(),
            sample_rate_hz: 48_000,
            buffer_frames: Some(256),
            output_gain_db: DEFAULT_OUTPUT_GAIN_DB,
            input_device: Some("Guitar interface".into()),
            input_channels: vec![1],
            input_gain_db: 3,
            midi_inputs: vec!["Keyboard".into()],
        };
        let text = toml::to_string(&preferences).unwrap();
        assert_eq!(
            toml::from_str::<AudioPreferences>(&text).unwrap(),
            preferences
        );
    }

    #[test]
    fn preferences_are_persisted_and_loaded() {
        let root = std::env::temp_dir().join(format!(
            "rackforge-audio-preferences-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("config/audio.toml");
        let preferences = AudioPreferences {
            schema_version: AUDIO_SCHEMA_VERSION,
            driver: "WASAPI".into(),
            output_device: "Test output".into(),
            sample_rate_hz: 48_000,
            buffer_frames: None,
            output_gain_db: DEFAULT_OUTPUT_GAIN_DB,
            input_device: None,
            input_channels: Vec::new(),
            input_gain_db: DEFAULT_INPUT_GAIN_DB,
            midi_inputs: vec!["Test MIDI".into()],
        };
        preferences.persist(&path).unwrap();
        assert_eq!(AudioPreferences::load(&path).unwrap(), Some(preferences));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stereo_is_mapped_to_multichannel_without_duplication() {
        let rendered = [0.25, -0.5, 0.75, -1.0];
        let mut target = vec![0.0_f32; 8];
        write_samples(&mut target, &rendered, 4, 1.0).unwrap();
        assert_eq!(target, [0.25, -0.5, 0.0, 0.0, 0.75, -1.0, 0.0, 0.0]);
    }

    #[test]
    fn output_gain_is_applied_and_clamped() {
        let rendered = [0.25, -0.75];
        let mut target = [0.0_f32; 2];
        write_samples(&mut target, &rendered, 2, 2.0).unwrap();
        assert_eq!(target, [0.5, -1.0]);
    }

    #[test]
    fn legacy_preferences_receive_the_desktop_gain_default() {
        let text = r#"
schema_version = 1
driver = "WASAPI"
output_device = "Speakers"
sample_rate_hz = 48000
midi_inputs = []
"#;
        let preferences: AudioPreferences = toml::from_str(text).unwrap();
        assert_eq!(preferences.output_gain_db, DEFAULT_OUTPUT_GAIN_DB);
    }

    #[test]
    fn audio_fallback_keeps_saved_midi_devices_for_hotplug() {
        let defaults = AudioPreferences {
            schema_version: AUDIO_SCHEMA_VERSION,
            driver: "WASAPI".into(),
            output_device: "System speakers".into(),
            sample_rate_hz: 48_000,
            buffer_frames: None,
            output_gain_db: DEFAULT_OUTPUT_GAIN_DB,
            input_device: None,
            input_channels: Vec::new(),
            input_gain_db: DEFAULT_INPUT_GAIN_DB,
            midi_inputs: Vec::new(),
        };
        let saved = AudioPreferences {
            schema_version: AUDIO_SCHEMA_VERSION,
            driver: "ASIO".into(),
            output_device: "Disconnected interface".into(),
            sample_rate_hz: 44_100,
            buffer_frames: Some(128),
            output_gain_db: DEFAULT_OUTPUT_GAIN_DB,
            input_device: Some("Disconnected interface".into()),
            input_channels: vec![1],
            input_gain_db: 0,
            midi_inputs: vec!["KL Essential 61 mk3 MIDI".into()],
        };

        let fallback = fallback_preserving_midi(defaults, &saved);

        assert_eq!(fallback.driver, "WASAPI");
        assert_eq!(fallback.output_device, "System speakers");
        assert_eq!(fallback.midi_inputs, saved.midi_inputs);
        assert_eq!(fallback.input_device, None);
    }

    #[test]
    fn capture_ring_publishes_complete_frames_and_reports_pressure() {
        let ring = CaptureRing::new(4);
        ring.push_frame(&[0.1, 0.2]);
        ring.push_frame(&[0.3, 0.4]);
        ring.push_frame(&[0.5, 0.6]);
        assert_eq!(ring.overruns.load(Ordering::Relaxed), 1);
        assert_eq!(ring.pop(), 0.1);
        assert_eq!(ring.pop(), 0.2);
        assert_eq!(ring.pop(), 0.3);
        assert_eq!(ring.pop(), 0.4);
        assert_eq!(ring.pop(), 0.0);
        assert_eq!(ring.underruns.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn certified_keylab_surface_is_auto_connected_without_a_saved_selection() {
        let selected = BTreeSet::from(["Other keyboard".to_owned()]);
        let present = BTreeSet::from([
            "Other keyboard".to_owned(),
            "KL Essential 61 mk3 MIDI".to_owned(),
            "KL Essential 61 mk3 MCU/HUI".to_owned(),
        ]);

        assert_eq!(
            desired_midi_inputs(&selected, &present, false),
            BTreeSet::from([
                "Other keyboard".to_owned(),
                "KL Essential 61 mk3 MIDI".to_owned(),
            ])
        );
    }

    #[test]
    fn callback_telemetry_reports_load_and_deadlines() {
        let telemetry = AudioTelemetry::default();
        telemetry.record_callback(48, 48_000, Duration::from_micros(500));
        telemetry.record_callback(48, 48_000, Duration::from_micros(1_500));
        let status = telemetry.snapshot(48_000);
        assert_eq!(status.callback_count, 2);
        assert_eq!(status.average_frames, 48.0);
        assert_eq!(status.average_callback_us, 1_000.0);
        assert_eq!(status.maximum_callback_us, 1_500.0);
        assert_eq!(status.callback_budget_us, 1_000.0);
        assert_eq!(status.callback_overruns, 1);
    }

    #[test]
    fn midi_disconnect_releases_sustain_before_all_notes() {
        let (sender, receiver) = mpsc::sync_channel(64);
        let telemetry = AudioTelemetry::default();
        release_held_notes(&sender, &telemetry);
        let packets = receiver.try_iter().collect::<Vec<_>>();
        assert_eq!(packets.len(), 32);
        for (channel, pair) in packets.as_chunks::<2>().0.iter().enumerate() {
            assert_eq!(pair[0].data, [0xb0 | channel as u8, 64, 0]);
            assert_eq!(pair[1].data, [0xb0 | channel as u8, 123, 0]);
        }
        assert_eq!(telemetry.midi_panic_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn midi_runtime_key_is_derived_from_persistent_identity() {
        let name = "KL Essential 61 mk3 MIDI";
        let id = stable_midi_source_id(name).unwrap();
        assert_eq!(
            stable_midi_source_key(name),
            stable_midi_source_key_from_id(&id)
        );
        assert_ne!(
            stable_midi_source_key(name),
            stable_midi_source_key("Other keyboard")
        );
    }
}
