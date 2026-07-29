use crate::control::{self, ControlCommand};
use crate::{LoadedPlugin, PluginInstance, PluginPackage};
use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use anyhow::{Context, Result, bail};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiInputPort};
use rackforge_control_api::{
    CONTROL_SCHEMA_VERSION, CONTROL_SOCKET_NAME, LiveSnapshot, SoundSummary,
};
use rackforge_plugin_api::PluginKind;
use rackforge_plugin_api::abi::MidiEventV1;
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

const OUTPUT_DEVICE: &str = "hw:CARD=USB,DEV=0";
const OUTPUT_RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const PERIOD_FRAMES: usize = 128;
const BUFFER_FRAMES: i64 = 384;
const MAX_EVENTS_PER_BLOCK: usize = 256;

pub struct LiveConfig {
    pub package: PathBuf,
    pub binary: Option<PathBuf>,
    pub resources: BTreeMap<String, PathBuf>,
    pub preset: Option<String>,
    pub data_root: Option<PathBuf>,
}

pub fn run(config: LiveConfig) -> Result<()> {
    let package = PluginPackage::open(&config.package)?;
    if package.manifest().kind != PluginKind::Instrument {
        bail!(
            "LIVE currently requires an instrument plugin, found {:?}",
            package.manifest().kind
        );
    }
    // SAFETY: LIVE is an explicit command that executes an installed native
    // plugin package.
    let plugin = unsafe {
        LoadedPlugin::load(
            &package,
            config.binary.as_deref(),
            &config.resources,
            config.data_root.as_deref(),
        )
    }?;
    println!(
        "LIVE_PLUGIN_READY id={} parameters={} presets={}",
        plugin.descriptor().id,
        plugin.parameters().parameters.len(),
        plugin.presets().presets.len()
    );
    let mut instance = plugin.create_instance()?;
    let presets = instance.preset_catalog()?;
    let preset = match config.preset.as_deref() {
        Some(id) => Some(
            presets
                .presets
                .iter()
                .chain(plugin.presets().presets.iter())
                .find(|preset| preset.id == id)
                .with_context(|| format!("plugin does not declare preset {id:?}"))?,
        ),
        None => presets.presets.first(),
    };
    if let Some(preset) = preset {
        instance.load_preset(&preset.id)?;
        println!("LIVE_PRESET_READY id={} name={:?}", preset.id, preset.name);
    }
    instance.activate(
        f64::from(OUTPUT_RATE),
        PERIOD_FRAMES as u32,
        0,
        CHANNELS as u32,
    )?;

    let (sender, receiver) = mpsc::channel();
    let (_midi_connection, midi_port_name) = connect_keylab(sender)?;
    println!("MIDI_READY port={midi_port_name:?}");
    let pcm = open_scarlett()?;
    println!(
        "AUDIO_READY device={OUTPUT_DEVICE:?} rate={OUTPUT_RATE} channels={CHANNELS} \
         format=S32_LE period={PERIOD_FRAMES} buffer={BUFFER_FRAMES}"
    );
    let selected_sound_id = preset
        .and_then(|selected| {
            presets
                .presets
                .iter()
                .find(|candidate| candidate.id == selected.id)
        })
        .map(|selected| selected.id.clone())
        .or_else(|| presets.presets.first().map(|preset| preset.id.clone()));
    let snapshot = Arc::new(Mutex::new(LiveSnapshot {
        schema_version: CONTROL_SCHEMA_VERSION,
        plugin_id: plugin.manifest().id.clone(),
        plugin_name: plugin.manifest().name.clone(),
        sounds: presets
            .presets
            .iter()
            .map(|preset| SoundSummary {
                id: preset.id.clone(),
                name: preset.name.clone(),
                detail: preset
                    .description
                    .clone()
                    .or_else(|| preset.category.clone()),
            })
            .collect(),
        selected_sound_id,
    }));
    let (control_sender, control_receiver) = mpsc::channel();
    let control_path = control_socket_path();
    let _control_server = control::start(&control_path, Arc::clone(&snapshot), control_sender)?;
    println!("CONTROL_READY socket={}", control_path.display());
    println!("READY_TO_PLAY");
    audio_loop(&pcm, &receiver, &control_receiver, &snapshot, &mut instance)
}

fn control_socket_path() -> PathBuf {
    env::var_os("RACKFORGE_CONTROL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let root = env::var_os("RACKFORGE_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/home/kalex/rackforge"));
            root.join("state").join(CONTROL_SOCKET_NAME)
        })
}

fn is_keylab_midi(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    (folded.contains("kl essential") || folded.contains("keylab"))
        && folded.contains("midi")
        && !folded.contains("dinthru")
        && !folded.contains("mcu")
        && !folded.contains("hui")
        && !folded.contains(" alv")
}

fn select_keylab_port(midi: &MidiInput) -> Result<(MidiInputPort, String)> {
    let mut matches = Vec::new();
    for port in midi.ports() {
        let name = midi.port_name(&port)?;
        if is_keylab_midi(&name) {
            matches.push((port, name));
        }
    }
    match matches.len() {
        0 => bail!("KeyLab main MIDI input was not found"),
        1 => Ok(matches.remove(0)),
        _ => bail!(
            "KeyLab MIDI input selection is ambiguous: {}",
            matches
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn connect_keylab(sender: Sender<MidiEventV1>) -> Result<(MidiInputConnection<()>, String)> {
    let mut midi = MidiInput::new("rackforge-core-live")?;
    midi.ignore(Ignore::None);
    let (port, name) = select_keylab_port(&midi)?;
    let connection = midi
        .connect(
            &port,
            "rackforge-core-live-input",
            move |_timestamp, message, _| {
                if message.is_empty() || message[0] >= 0xF0 || message.len() > 3 {
                    return;
                }
                let mut data = [0_u8; 3];
                data[..message.len()].copy_from_slice(message);
                let _ = sender.send(MidiEventV1 {
                    frame: 0,
                    length: message.len() as u8,
                    data,
                });
            },
            (),
        )
        .map_err(|error| anyhow::anyhow!("connecting KeyLab MIDI input: {error}"))?;
    Ok((connection, name))
}

fn open_scarlett() -> Result<PCM> {
    let pcm = PCM::new(OUTPUT_DEVICE, Direction::Playback, false)
        .context("opening Scarlett ALSA playback")?;
    {
        let parameters = HwParams::any(&pcm)?;
        parameters.set_access(Access::RWInterleaved)?;
        parameters.set_format(Format::s32())?;
        parameters.set_channels(CHANNELS as u32)?;
        parameters.set_rate(OUTPUT_RATE, ValueOr::Nearest)?;
        parameters.set_period_size(PERIOD_FRAMES as i64, ValueOr::Nearest)?;
        parameters.set_buffer_size(BUFFER_FRAMES)?;
        pcm.hw_params(&parameters)?;
    }
    let actual = pcm.hw_params_current()?;
    if actual.get_rate()? != OUTPUT_RATE
        || actual.get_channels()? != CHANNELS as u32
        || actual.get_period_size()? != PERIOD_FRAMES as i64
    {
        bail!(
            "Scarlett negotiated unsupported format: rate={} channels={} period={}",
            actual.get_rate()?,
            actual.get_channels()?,
            actual.get_period_size()?
        );
    }
    drop(actual);
    pcm.prepare()?;
    Ok(pcm)
}

fn audio_loop(
    pcm: &PCM,
    receiver: &Receiver<MidiEventV1>,
    control_receiver: &Receiver<ControlCommand>,
    snapshot: &Arc<Mutex<LiveSnapshot>>,
    instance: &mut PluginInstance<'_>,
) -> Result<()> {
    let io = pcm.io_i32()?;
    let input = Vec::new();
    let mut plugin_output = vec![0.0_f32; PERIOD_FRAMES * CHANNELS];
    let mut device_output = vec![0_i32; PERIOD_FRAMES * CHANNELS];
    let mut events = Vec::with_capacity(MAX_EVENTS_PER_BLOCK);
    let mut meter_frames = 0_usize;
    let mut meter_peak = 0_f32;
    let mut meter_clipped = 0_usize;
    let mut dropped_events = 0_usize;

    loop {
        while let Ok(command) = control_receiver.try_recv() {
            match command {
                ControlCommand::SelectSound { id, reply } => {
                    let result = instance.load_preset(&id).map(|()| {
                        if let Ok(mut snapshot) = snapshot.lock() {
                            snapshot.selected_sound_id = Some(id.clone());
                        }
                        println!("LIVE_SOUND_SELECTED id={id}");
                        id
                    });
                    let _ = reply.send(result.map_err(|error| error.to_string()));
                }
            }
        }
        events.clear();
        while let Ok(event) = receiver.try_recv() {
            if events.len() < MAX_EVENTS_PER_BLOCK {
                events.push(event);
            } else {
                dropped_events += 1;
            }
        }

        plugin_output.fill(0.0);
        instance.process_interleaved(
            &input,
            &mut plugin_output,
            PERIOD_FRAMES as u32,
            0,
            CHANNELS as u32,
            &events,
            &[],
        )?;
        for (target, sample) in device_output.iter_mut().zip(&plugin_output) {
            meter_peak = meter_peak.max(sample.abs());
            meter_clipped += usize::from(sample.abs() > 0.95);
            *target = (sample.clamp(-0.95, 0.95) * i32::MAX as f32) as i32;
        }
        meter_frames += PERIOD_FRAMES;
        write_period(pcm, &io, &device_output)?;

        if meter_frames >= OUTPUT_RATE as usize {
            println!(
                "AUDIO_METER peak={meter_peak:.3} clipped={meter_clipped} \
                 midi_events={} dropped_events={dropped_events}",
                events.len()
            );
            meter_frames = 0;
            meter_peak = 0.0;
            meter_clipped = 0;
            dropped_events = 0;
        }
    }
}

fn write_period(pcm: &PCM, io: &alsa::pcm::IO<'_, i32>, output: &[i32]) -> Result<()> {
    let mut frame_offset = 0;
    while frame_offset < PERIOD_FRAMES {
        match io.writei(&output[frame_offset * CHANNELS..]) {
            Ok(0) => bail!("Scarlett accepted zero audio frames"),
            Ok(frames) => frame_offset += frames,
            Err(error) if error.errno() == libc::EPIPE => {
                eprintln!("XRUN_RECOVERED");
                pcm.prepare()?;
                frame_offset = 0;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_only_the_main_keylab_endpoint() {
        assert!(is_keylab_midi("KL Essential 61 mk3 MIDI 28:0"));
        assert!(!is_keylab_midi("KL Essential 61 mk3 DINTHRU 28:1"));
        assert!(!is_keylab_midi("KL Essential 61 mk3 MCU/HUI 28:2"));
    }
}
