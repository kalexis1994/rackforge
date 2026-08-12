use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample, SupportedBufferSize};
use keylab_essential_mk3::{controller as keylab_controller, protocol as keylab_protocol};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use rackforge_core::{
    LoadedPlugin, PluginInstance,
    midi_hotplug::{PanicScope, panic_packets},
};
use rackforge_plugin_api::{PreparedProgram, PresetCatalog, abi::MidiEventV1};
use rackforge_session_api::{HostControlTarget, MasterLevel, MasterPan};
use rackforge_surface_runtime::{Input as SurfaceInput, Screen};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const AUDIO_SCHEMA_VERSION: u32 = 1;
const PLUGIN_OUTPUT_CHANNELS: usize = 2;
const MAX_AUDIO_FRAMES: usize = 4_096;
const MIDI_QUEUE_CAPACITY: usize = 4_096;
const COMMAND_QUEUE_CAPACITY: usize = 64;
const CONTROLLER_QUEUE_CAPACITY: usize = 256;
const DISPLAY_QUEUE_CAPACITY: usize = 8;
const MAX_MIDI_EVENTS_PER_BLOCK: usize = 4_096;
const COMMON_SAMPLE_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
const COMMON_BUFFER_FRAMES: [u32; 8] = [32, 64, 128, 256, 512, 1_024, 2_048, 4_096];
const DEFAULT_OUTPUT_GAIN_DB: i8 = 6;
const MAX_OUTPUT_GAIN_DB: i8 = 12;
const MIDI_RECONNECT_INTERVAL: Duration = Duration::from_secs(1);
const MIDI_SUPERVISOR_TICK: Duration = Duration::from_millis(20);
const MASTER_SMOOTHING_FRAMES: u32 = 480;
const CONTROL_RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

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
        }
    }
}

pub struct VoiceSpec {
    pub instance_id: String,
    pub plugin: &'static LoadedPlugin,
    pub preset_id: Option<String>,
    pub resources: BTreeMap<String, PathBuf>,
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
    pub midi_inputs: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct AudioDriverInfo {
    pub name: String,
    pub available: bool,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct AudioOutputInfo {
    pub driver: String,
    pub name: String,
    pub is_default: bool,
    pub channels: u16,
    pub default_sample_rate: u32,
    pub sample_rates: Vec<u32>,
    pub buffer_frames: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct AudioInventory {
    pub drivers: Vec<AudioDriverInfo>,
    pub outputs: Vec<AudioOutputInfo>,
    pub midi_inputs: Vec<String>,
}

impl AudioInventory {
    pub fn scan() -> Result<Self> {
        let mut drivers = Vec::new();
        let mut outputs = Vec::new();
        for host_id in cpal::available_hosts() {
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
            let mut driver_outputs = 0usize;
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
            drivers.push(AudioDriverInfo {
                name: driver,
                available: driver_outputs > 0,
                detail: if driver_outputs == 0 {
                    "No output devices found".into()
                } else {
                    format!("{driver_outputs} output device(s)")
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
        let midi_inputs = discover_midi_inputs()?;
        Ok(Self {
            drivers,
            outputs,
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
            midi_inputs: self.midi_inputs.clone(),
        })
    }

    pub fn output(&self, preferences: &AudioPreferences) -> Option<&AudioOutputInfo> {
        self.outputs.iter().find(|output| {
            output.driver == preferences.driver && output.name == preferences.output_device
        })
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

pub struct DesktopAudio {
    _stream: cpal::Stream,
    _midi_supervisor: MidiSupervisor,
    command_sender: SyncSender<AudioCommand>,
    controller_receiver: Receiver<DesktopControllerEvent>,
    display_sender: SyncSender<Screen>,
    errors: Arc<Mutex<Option<String>>>,
    telemetry: Arc<AudioTelemetry>,
    sample_rate: u32,
    summary: String,
    _voice_reclaimer: thread::JoinHandle<()>,
}

impl DesktopAudio {
    pub fn start(
        specs: Vec<VoiceSpec>,
        preferences: &AudioPreferences,
        active_instance_id: Option<&str>,
    ) -> Result<Self> {
        if specs.is_empty() {
            bail!("no instrument plugin is available for the audio engine");
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

        let voice_capacity = specs.len().saturating_add(8);
        let mut voices = Vec::with_capacity(specs.len());
        for spec in specs {
            voices.push(prepare_audio_voice(spec, config.sample_rate.0)?);
        }
        let active_voice = active_instance_id
            .and_then(|active| voices.iter().position(|voice| voice.instance_id == active))
            .unwrap_or(0);

        let telemetry = Arc::new(AudioTelemetry::default());
        let (midi_sender, midi_receiver) = mpsc::sync_channel(MIDI_QUEUE_CAPACITY);
        let (controller_sender, controller_receiver) =
            mpsc::sync_channel(CONTROLLER_QUEUE_CAPACITY);
        let (display_sender, display_receiver) = mpsc::sync_channel(DISPLAY_QUEUE_CAPACITY);
        let (midi_supervisor, midi_names) = MidiSupervisor::start(
            midi_sender,
            preferences.midi_inputs.clone(),
            Arc::clone(&telemetry),
            controller_sender,
            display_receiver,
        )?;
        let (command_sender, command_receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let (retired_voice_sender, retired_voice_receiver) = mpsc::sync_channel(voice_capacity);
        let voice_reclaimer = thread::Builder::new()
            .name("rackforge-audio-voice-reclaimer".into())
            .spawn(move || while retired_voice_receiver.recv().is_ok() {})?;
        let errors = Arc::new(Mutex::new(None));
        let callback_errors = Arc::clone(&errors);
        let stream_errors = Arc::clone(&errors);
        let callback_telemetry = Arc::clone(&telemetry);
        let stream_telemetry = Arc::clone(&telemetry);
        let callback_sample_rate = config.sample_rate.0;
        let callback_channels = device_channels;
        let mut processor = AudioProcessor {
            voices,
            active_voice,
            midi_receiver,
            command_receiver,
            events: Vec::with_capacity(MAX_MIDI_EVENTS_PER_BLOCK),
            output: vec![0.0; MAX_AUDIO_FRAMES * PLUGIN_OUTPUT_CHANNELS],
            device_channels,
            output_gain: db_to_amplitude(preferences.output_gain_db),
            master_gain: MasterGain::new(MasterLevel::UNITY),
            master_balance: MasterBalance::new(MasterPan::CENTER),
            stopped: false,
            retired_voice_sender,
            deferred_retire: Vec::with_capacity(voice_capacity),
        };
        let stream = device
            .build_output_stream_raw(
                &config,
                sample_format,
                move |data, _| {
                    let started = Instant::now();
                    let frames = data.len() / callback_channels;
                    if let Err(error) = render_output(&mut processor, data, sample_format) {
                        silence_output(data, sample_format);
                        publish_error(&callback_errors, error);
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
            "{} · {} · {} Hz · {buffer} · {} ch · {:+} dB · MIDI: {midi_summary}",
            preferences.driver,
            preferences.output_device,
            config.sample_rate.0,
            config.channels,
            preferences.output_gain_db
        );
        println!("DESKTOP_AUDIO_READY {summary}");
        Ok(Self {
            _stream: stream,
            _midi_supervisor: midi_supervisor,
            command_sender,
            controller_receiver,
            display_sender,
            errors,
            telemetry,
            sample_rate: config.sample_rate.0,
            summary,
            _voice_reclaimer: voice_reclaimer,
        })
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    pub fn runtime_status(&self) -> AudioRuntimeStatus {
        self.telemetry.snapshot(self.sample_rate)
    }

    pub fn diagnostics(&self) -> String {
        let status = self.runtime_status();
        format!(
            "{}\nCallback: {} blocks · {:.1}% CPU · avg {:.1} µs · max {:.1} µs · budget {:.1} µs · {:.0} frames\nDeadlines missed: {} · MIDI dropped: {} · disconnect panics: {} · stream errors: {}",
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

    pub fn replace_voice(&self, spec: VoiceSpec) -> Result<()> {
        let voice = prepare_audio_voice(spec, self.sample_rate)?;
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

    pub fn emergency_stop(&self) -> Result<()> {
        self.send_command(AudioCommand::EmergencyStop)
    }

    pub fn try_controller_event(&self) -> Option<DesktopControllerEvent> {
        self.controller_receiver.try_recv().ok()
    }

    pub fn render_little(&self, screen: Screen) {
        match self.display_sender.try_send(screen) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(screen)) => {
                // The supervisor will shortly drain the queue. Losing an intermediate
                // animation frame is preferable to ever blocking the UI/audio path.
                let _ = screen;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {}
        }
    }

    pub fn test_note(&self) -> Result<()> {
        self.send_command(AudioCommand::InjectMidi(MidiPacket {
            length: 3,
            data: [0x90, 60, 100],
        }))?;
        let sender = self.command_sender.clone();
        thread::Builder::new()
            .name("rackforge-audio-test-note".into())
            .spawn(move || {
                thread::sleep(Duration::from_millis(350));
                let _ = sender.try_send(AudioCommand::InjectMidi(MidiPacket {
                    length: 3,
                    data: [0x80, 60, 0],
                }));
            })?;
        Ok(())
    }

    pub fn take_error(&self) -> Option<String> {
        self.errors.lock().ok()?.take()
    }

    fn send_command(&self, command: AudioCommand) -> Result<()> {
        self.command_sender
            .try_send(command)
            .map_err(|error| anyhow::anyhow!("audio command queue rejected command: {error}"))
    }
}

enum AudioCommand {
    SelectPlugin(String),
    SelectSound {
        instance_id: String,
        sound_id: String,
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
    InjectMidi(MidiPacket),
    SetMasterLevel(MasterLevel),
    SetMasterPan(MasterPan),
    SetRunning(bool),
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopControllerEvent {
    Connected,
    Disconnected,
    Surface {
        input: SurfaceInput,
        phase: keylab_protocol::InputPhase,
    },
    MasterLevel(u8),
    MasterPan(u8),
}

#[derive(Clone, Copy)]
struct MidiPacket {
    length: u8,
    data: [u8; 3],
}

struct AudioVoice {
    instance_id: String,
    instance: SendablePluginInstance,
}

struct SendablePluginInstance(PluginInstance<'static>);

// SAFETY: the instance is moved once into CPAL's output callback, and every
// subsequent plugin ABI call happens serially on that callback. RackForge does
// not share the handle with the UI instance or access it after the move.
unsafe impl Send for SendablePluginInstance {}

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
    output: Vec<f32>,
    device_channels: usize,
    output_gain: f32,
    master_gain: MasterGain,
    master_balance: MasterBalance,
    stopped: bool,
    retired_voice_sender: SyncSender<AudioVoice>,
    deferred_retire: Vec<AudioVoice>,
}

impl AudioProcessor {
    fn render(&mut self, frames: usize) -> Result<&[f32]> {
        if frames == 0 || frames > MAX_AUDIO_FRAMES {
            bail!("Windows requested an unsupported audio block of {frames} frames");
        }
        self.events.clear();
        self.apply_commands()?;
        while self.events.len() < MAX_MIDI_EVENTS_PER_BLOCK {
            let Ok(packet) = self.midi_receiver.try_recv() else {
                break;
            };
            push_midi_event(&mut self.events, packet);
        }
        let samples = frames * PLUGIN_OUTPUT_CHANNELS;
        let output = &mut self.output[..samples];
        output.fill(0.0);
        if self.stopped {
            return Ok(output);
        }
        self.voices[self.active_voice]
            .instance
            .0
            .process_interleaved(
                &[],
                output,
                frames as u32,
                0,
                PLUGIN_OUTPUT_CHANNELS as u32,
                &self.events,
                &[],
            )
            .context("processing RackForge plugin audio")?;
        for frame in output.chunks_exact_mut(PLUGIN_OUTPUT_CHANNELS) {
            let gain = self.master_gain.next();
            let (left, right) = self.master_balance.next();
            frame[0] *= gain * left;
            frame[1] *= gain * right;
        }
        Ok(output)
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
                        self.voices[self.active_voice].instance.0.reset()?;
                        self.active_voice = index;
                    }
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
                    self.voices[index].instance.0.load_preset(&sound_id)?;
                    if index != self.active_voice {
                        self.voices[self.active_voice].instance.0.reset()?;
                        self.active_voice = index;
                    }
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
                                .instance
                                .0
                                .reset()
                                .map_err(|error| error.to_string())?;
                        }
                        let previewed = self.voices[index]
                            .instance
                            .0
                            .preview_program(&prepared)
                            .map_err(|error| error.to_string())?;
                        if !previewed {
                            self.voices[index]
                                .instance
                                .0
                                .load_preset(&prepared.preview_sound_id)
                                .map_err(|error| error.to_string())?;
                        }
                        if index != self.active_voice {
                            self.voices[self.active_voice]
                                .instance
                                .0
                                .reset()
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
                            .instance
                            .0
                            .install_program(&prepared)
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
                            .instance
                            .0
                            .reset()
                            .map_err(|error| error.to_string())?;
                        if let Some(sound_id) = sound_id {
                            self.voices[index]
                                .instance
                                .0
                                .load_preset(&sound_id)
                                .map_err(|error| error.to_string())?;
                        }
                        if index != self.active_voice {
                            self.voices[self.active_voice]
                                .instance
                                .0
                                .reset()
                                .map_err(|error| error.to_string())?;
                            self.active_voice = index;
                        }
                        Ok(())
                    })();
                    let _ = reply.try_send(result);
                }
                AudioCommand::InjectMidi(packet) => push_midi_event(&mut self.events, packet),
                AudioCommand::SetMasterLevel(level) => self.master_gain.set_level(level),
                AudioCommand::SetMasterPan(pan) => self.master_balance.set_pan(pan),
                AudioCommand::SetRunning(running) => self.stopped = !running,
                AudioCommand::EmergencyStop => {
                    for voice in &mut self.voices {
                        voice.instance.0.reset()?;
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

fn prepare_audio_voice(spec: VoiceSpec, sample_rate: u32) -> Result<AudioVoice> {
    let mut instance = spec
        .plugin
        .create_instance_with_resource_overrides(&spec.resources)
        .with_context(|| format!("creating audio instance {}", spec.instance_id))?;
    if let Some(preset_id) = spec.preset_id.as_deref() {
        instance
            .load_preset(preset_id)
            .with_context(|| format!("loading preset {preset_id:?} for the audio engine"))?;
    }
    instance
        .activate(
            f64::from(sample_rate),
            MAX_AUDIO_FRAMES as u32,
            0,
            PLUGIN_OUTPUT_CHANNELS as u32,
        )
        .with_context(|| format!("activating audio instance {}", spec.instance_id))?;
    Ok(AudioVoice {
        instance_id: spec.instance_id,
        instance: SendablePluginInstance(instance),
    })
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

fn render_output(
    processor: &mut AudioProcessor,
    data: &mut cpal::Data,
    format: SampleFormat,
) -> Result<()> {
    let frames = data.len() / processor.device_channels;
    let device_channels = processor.device_channels;
    let output_gain = processor.output_gain;
    let rendered = processor.render(frames)?;
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
        display_receiver: Receiver<Screen>,
    ) -> Result<(Self, Vec<String>)> {
        let selected = selected.into_iter().collect::<BTreeSet<_>>();
        let (stop_sender, stop_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("rackforge-desktop-midi-supervisor".into())
            .spawn(move || {
                let mut connections = BTreeMap::new();
                let mut display = None;
                let mut latest_screen = None;
                let mut display_dirty = false;
                match reconcile_midi_inputs(
                    &selected,
                    &mut connections,
                    &sender,
                    &telemetry,
                    &controller_sender,
                ) {
                    Ok(names) => {
                        if reconcile_keylab_display(&selected, &mut display, latest_screen.as_ref())
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
                    while let Ok(screen) = display_receiver.try_recv() {
                        latest_screen = Some(screen);
                        display_dirty = true;
                    }
                    if display_dirty
                        && let (Some(display), Some(screen)) =
                            (display.as_mut(), latest_screen.as_ref())
                        && let Err(error) = display.render(screen)
                    {
                        eprintln!("DESKTOP_KEYLAB_DISPLAY_FAILED error={error:#}");
                        display.restore_best_effort();
                    }
                    display_dirty = false;
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
                        ) {
                            eprintln!("DESKTOP_MIDI_SCAN_FAILED error={error:#}");
                        }
                        if reconcile_keylab_display(&selected, &mut display, latest_screen.as_ref())
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
) -> Result<Vec<String>> {
    let present = discover_midi_inputs()?.into_iter().collect::<BTreeSet<_>>();
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
    for name in selected {
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
    midi.connect(
        &port,
        "rackforge-desktop-input",
        move |_timestamp, message, _| {
            if keylab && let Some(event) = keylab_protocol::parse_input(message) {
                let event = match event {
                    keylab_protocol::ControllerEvent::Surface { input, phase } => {
                        DesktopControllerEvent::Surface { input, phase }
                    }
                    keylab_protocol::ControllerEvent::HostControl {
                        target: HostControlTarget::MasterLevel,
                        value,
                    } => DesktopControllerEvent::MasterLevel(value),
                    keylab_protocol::ControllerEvent::HostControl {
                        target: HostControlTarget::MasterPan,
                        value,
                    } => DesktopControllerEvent::MasterPan(value),
                };
                let _ = controller_sender.try_send(event);
                return;
            }
            if message.is_empty() || message.len() > 3 {
                return;
            }
            let mut data = [0; 3];
            data[..message.len()].copy_from_slice(message);
            if sender
                .try_send(MidiPacket {
                    length: message.len() as u8,
                    data,
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

    fn render(&mut self, screen: &Screen) -> Result<()> {
        let messages = keylab_protocol::render_messages(screen).map_err(anyhow::Error::msg)?;
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
    selected: &BTreeSet<String>,
    display: &mut Option<KeyLabDisplay>,
    latest_screen: Option<&Screen>,
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
    let Some(name) = selected.iter().find(|name| {
        keylab_controller::little_driver(name).is_some() && outputs.contains(name.as_str())
    }) else {
        return false;
    };
    match KeyLabDisplay::open(name) {
        Ok(mut opened) => {
            if let Some(screen) = latest_screen
                && let Err(error) = opened.render(screen)
            {
                eprintln!("DESKTOP_KEYLAB_INITIAL_RENDER_FAILED error={error:#}");
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
    if let Err(error) =
        reconcile_midi_inputs(selected, connections, sender, telemetry, controller_sender)
    {
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
                length: packet.length,
                data: packet.data,
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
        for (channel, pair) in packets.chunks_exact(2).enumerate() {
            assert_eq!(pair[0].data, [0xb0 | channel as u8, 64, 0]);
            assert_eq!(pair[1].data, [0xb0 | channel as u8, 123, 0]);
        }
        assert_eq!(telemetry.midi_panic_count.load(Ordering::Relaxed), 1);
    }
}
