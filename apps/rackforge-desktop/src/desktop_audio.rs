use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, FromSample, Sample, SampleFormat, SizedSample, SupportedBufferSize};
use midir::{Ignore, MidiInput, MidiInputConnection};
use rackforge_core::{LoadedPlugin, PluginInstance};
use rackforge_plugin_api::abi::MidiEventV1;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const AUDIO_SCHEMA_VERSION: u32 = 1;
const PLUGIN_OUTPUT_CHANNELS: usize = 2;
const MAX_AUDIO_FRAMES: usize = 4_096;
const MIDI_QUEUE_CAPACITY: usize = 4_096;
const COMMAND_QUEUE_CAPACITY: usize = 64;
const MAX_MIDI_EVENTS_PER_BLOCK: usize = 4_096;
const COMMON_SAMPLE_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
const COMMON_BUFFER_FRAMES: [u32; 8] = [32, 64, 128, 256, 512, 1_024, 2_048, 4_096];
const DEFAULT_OUTPUT_GAIN_DB: i8 = 6;
const MAX_OUTPUT_GAIN_DB: i8 = 12;

pub struct VoiceSpec {
    pub instance_id: String,
    pub plugin: &'static LoadedPlugin,
    pub preset_id: Option<String>,
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
            .find(|output| output.is_default)
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
    _midi_connections: Vec<MidiInputConnection<()>>,
    command_sender: SyncSender<AudioCommand>,
    errors: Arc<Mutex<Option<String>>>,
    summary: String,
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

        let mut voices = Vec::with_capacity(specs.len());
        for spec in specs {
            let mut instance = spec
                .plugin
                .create_instance()
                .with_context(|| format!("creating audio instance {}", spec.instance_id))?;
            if let Some(preset_id) = spec.preset_id.as_deref() {
                instance.load_preset(preset_id).with_context(|| {
                    format!("loading preset {preset_id:?} for the audio engine")
                })?;
            }
            instance
                .activate(
                    f64::from(config.sample_rate.0),
                    MAX_AUDIO_FRAMES as u32,
                    0,
                    PLUGIN_OUTPUT_CHANNELS as u32,
                )
                .with_context(|| format!("activating audio instance {}", spec.instance_id))?;
            voices.push(AudioVoice {
                instance_id: spec.instance_id,
                instance: SendablePluginInstance(instance),
            });
        }
        let active_voice = active_instance_id
            .and_then(|active| voices.iter().position(|voice| voice.instance_id == active))
            .unwrap_or(0);

        let (midi_sender, midi_receiver) = mpsc::sync_channel(MIDI_QUEUE_CAPACITY);
        let (midi_connections, midi_names) =
            connect_midi_inputs(midi_sender, &preferences.midi_inputs)?;
        let (command_sender, command_receiver) = mpsc::sync_channel(COMMAND_QUEUE_CAPACITY);
        let errors = Arc::new(Mutex::new(None));
        let callback_errors = Arc::clone(&errors);
        let stream_errors = Arc::clone(&errors);
        let mut processor = AudioProcessor {
            voices,
            active_voice,
            midi_receiver,
            command_receiver,
            events: Vec::with_capacity(MAX_MIDI_EVENTS_PER_BLOCK),
            output: vec![0.0; MAX_AUDIO_FRAMES * PLUGIN_OUTPUT_CHANNELS],
            device_channels,
            output_gain: db_to_amplitude(preferences.output_gain_db),
        };
        let stream = device
            .build_output_stream_raw(
                &config,
                sample_format,
                move |data, _| {
                    if let Err(error) = render_output(&mut processor, data, sample_format) {
                        silence_output(data, sample_format);
                        publish_error(&callback_errors, error);
                    }
                },
                move |error| {
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
            _midi_connections: midi_connections,
            command_sender,
            errors,
            summary,
        })
    }

    pub fn summary(&self) -> &str {
        &self.summary
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
    InjectMidi(MidiPacket),
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

struct AudioProcessor {
    voices: Vec<AudioVoice>,
    active_voice: usize,
    midi_receiver: Receiver<MidiPacket>,
    command_receiver: Receiver<AudioCommand>,
    events: Vec<MidiEventV1>,
    output: Vec<f32>,
    device_channels: usize,
    output_gain: f32,
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
        Ok(output)
    }

    fn apply_commands(&mut self) -> Result<()> {
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
                AudioCommand::InjectMidi(packet) => push_midi_event(&mut self.events, packet),
            }
        }
        Ok(())
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

fn connect_midi_inputs(
    sender: SyncSender<MidiPacket>,
    selected: &[String],
) -> Result<(Vec<MidiInputConnection<()>>, Vec<String>)> {
    let available = discover_midi_inputs()?;
    let selected = selected.iter().collect::<BTreeSet<_>>();
    let names = available
        .into_iter()
        .filter(|name| selected.contains(name))
        .collect::<Vec<_>>();
    let mut connections = Vec::with_capacity(names.len());
    let mut connected_names = Vec::with_capacity(names.len());
    for (index, name) in names.into_iter().enumerate() {
        let mut midi = MidiInput::new(&format!("rackforge-desktop-midi-{index}"))
            .context("opening a Windows MIDI client")?;
        midi.ignore(Ignore::None);
        let Some(port) = midi
            .ports()
            .into_iter()
            .find(|port| midi.port_name(port).as_deref() == Ok(name.as_str()))
        else {
            continue;
        };
        let input_sender = sender.clone();
        let connection = midi
            .connect(
                &port,
                &format!("rackforge-desktop-input-{index}"),
                move |_timestamp, message, _| {
                    if message.is_empty() || message.len() > 3 {
                        return;
                    }
                    let mut data = [0; 3];
                    data[..message.len()].copy_from_slice(message);
                    let _ = input_sender.try_send(MidiPacket {
                        length: message.len() as u8,
                        data,
                    });
                },
                (),
            )
            .map_err(|error| anyhow::anyhow!("connecting Windows MIDI input {name:?}: {error}"))?;
        connections.push(connection);
        connected_names.push(name);
    }
    Ok((connections, connected_names))
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
}
