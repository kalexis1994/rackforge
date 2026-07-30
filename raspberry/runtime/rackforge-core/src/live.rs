use crate::control::{self, ControlCommand};
use crate::{LoadedPlugin, PluginInstance, PluginPackage};
use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use anyhow::{Context, Result, bail};
use midir::{Ignore, MidiInput, MidiInputConnection};
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
use std::time::{Duration, Instant};

const OUTPUT_DEVICE: &str = "hw:CARD=USB,DEV=0";
const OUTPUT_RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const PERIOD_FRAMES: usize = 128;
const BUFFER_FRAMES: i64 = 384;
const MAX_EVENTS_PER_BLOCK: usize = 256;
const AUDITION_LEASE_TIMEOUT: Duration = Duration::from_secs(15);

struct AuditionLease {
    id: u64,
    previous_sound_id: Option<String>,
    last_keep_alive: Instant,
}

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
    let (_midi_connections, midi_port_names) = connect_midi_sources(sender)?;
    println!("MIDI_READY ports={midi_port_names:?}");
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
        ui_layouts: plugin.manifest().ui_layouts.clone(),
        sounds: presets
            .presets
            .iter()
            .map(|preset| SoundSummary {
                id: preset.id.clone(),
                name: preset.name.clone(),
                bank: preset.bank.clone(),
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

fn is_performance_midi_input(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    folded.contains("midi")
        && !folded.contains("midi through")
        && !folded.contains("dinthru")
        && !folded.contains("mcu")
        && !folded.contains("hui")
        && !folded.contains(" alv")
        && !folded.contains("rackforge")
}

fn performance_midi_names(midi: &MidiInput) -> Result<Vec<String>> {
    let mut matches = BTreeMap::new();
    for port in midi.ports() {
        let name = midi.port_name(&port)?;
        if is_performance_midi_input(&name) {
            matches.insert(name.clone(), name);
        }
    }
    if matches.is_empty() {
        bail!("no performance MIDI input was found");
    }
    Ok(matches.into_values().collect())
}

fn connect_midi_sources(
    sender: Sender<MidiEventV1>,
) -> Result<(Vec<MidiInputConnection<()>>, Vec<String>)> {
    let discovery = MidiInput::new("rackforge-core-discovery")?;
    let names = performance_midi_names(&discovery)?;
    let mut connections = Vec::with_capacity(names.len());
    for (index, name) in names.iter().enumerate() {
        let mut midi = MidiInput::new(&format!("rackforge-core-live-{index}"))?;
        midi.ignore(Ignore::None);
        let port = midi
            .ports()
            .into_iter()
            .find(|port| midi.port_name(port).as_deref() == Ok(name.as_str()))
            .with_context(|| format!("MIDI input {name:?} disappeared during connection"))?;
        let source_sender = sender.clone();
        let connection = midi
            .connect(
                &port,
                &format!("rackforge-core-input-{index}"),
                move |_timestamp, message, _| {
                    if message.is_empty() || message[0] >= 0xF0 || message.len() > 3 {
                        return;
                    }
                    let mut data = [0_u8; 3];
                    data[..message.len()].copy_from_slice(message);
                    let _ = source_sender.send(MidiEventV1 {
                        frame: 0,
                        length: message.len() as u8,
                        data,
                    });
                },
                (),
            )
            .map_err(|error| anyhow::anyhow!("connecting MIDI input {name:?}: {error}"))?;
        connections.push(connection);
    }
    Ok((connections, names))
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
    let mut audition: Option<AuditionLease> = None;
    let mut next_audition_id = 1_u64;

    loop {
        if audition
            .as_ref()
            .is_some_and(|lease| lease.last_keep_alive.elapsed() >= AUDITION_LEASE_TIMEOUT)
            && let Some(expired) = audition.take()
        {
            if let Err(error) = restore_after_audition(instance, snapshot, &expired) {
                eprintln!(
                    "AUDITION_RESTORE_ERROR lease={} error={error:#}",
                    expired.id
                );
            } else {
                println!("AUDITION_EXPIRED lease={}", expired.id);
            }
        }
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
                ControlCommand::BeginAudition { plugin_id, reply } => {
                    let result = (|| -> Result<u64, String> {
                        if audition.is_some() {
                            return Err("audition focus is already leased".into());
                        }
                        let (active_plugin, previous_sound_id) = snapshot
                            .lock()
                            .map_err(|_| "live snapshot lock is poisoned".to_owned())
                            .map(|snapshot| {
                                (
                                    snapshot.plugin_id.clone(),
                                    snapshot.selected_sound_id.clone(),
                                )
                            })?;
                        if plugin_id != active_plugin {
                            return Err(format!(
                                "plugin {plugin_id:?} cannot audition while {active_plugin:?} is active"
                            ));
                        }
                        instance.reset().map_err(|error| error.to_string())?;
                        let lease_id = next_audition_id;
                        next_audition_id = next_audition_id.wrapping_add(1).max(1);
                        audition = Some(AuditionLease {
                            id: lease_id,
                            previous_sound_id,
                            last_keep_alive: Instant::now(),
                        });
                        println!("AUDITION_GRANTED lease={lease_id} plugin={plugin_id}");
                        Ok(lease_id)
                    })();
                    let _ = reply.send(result);
                }
                ControlCommand::KeepAuditionAlive { lease_id, reply } => {
                    let result = match audition.as_mut() {
                        Some(lease) if lease.id == lease_id => {
                            lease.last_keep_alive = Instant::now();
                            Ok(lease_id)
                        }
                        _ => Err("audition lease is missing or no longer valid".into()),
                    };
                    let _ = reply.send(result);
                }
                ControlCommand::EndAudition { lease_id, reply } => {
                    let result = match audition.take() {
                        Some(lease) if lease.id == lease_id => {
                            restore_after_audition(instance, snapshot, &lease)
                                .map_err(|error| error.to_string())
                                .map(|()| println!("AUDITION_RELEASED lease={lease_id}"))
                        }
                        Some(lease) => {
                            audition = Some(lease);
                            Err("audition lease is missing or no longer valid".into())
                        }
                        None => Err("audition lease is missing or no longer valid".into()),
                    };
                    let _ = reply.send(result);
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

fn restore_after_audition(
    instance: &mut PluginInstance<'_>,
    snapshot: &Arc<Mutex<LiveSnapshot>>,
    lease: &AuditionLease,
) -> Result<()> {
    instance.reset()?;
    if let Some(previous) = &lease.previous_sound_id {
        instance.load_preset(previous)?;
    }
    let mut snapshot = snapshot
        .lock()
        .map_err(|_| anyhow::anyhow!("live snapshot lock is poisoned"))?;
    snapshot.selected_sound_id = lease.previous_sound_id.clone();
    Ok(())
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
    fn accepts_unknown_musical_midi_without_treating_auxiliary_ports_as_sources() {
        assert!(is_performance_midi_input("KL Essential 61 mk3 MIDI 28:0"));
        assert!(is_performance_midi_input("Unknown USB MIDI 31:0"));
        assert!(!is_performance_midi_input(
            "KL Essential 61 mk3 DINTHRU 28:1"
        ));
        assert!(!is_performance_midi_input(
            "KL Essential 61 mk3 MCU/HUI 28:2"
        ));
        assert!(!is_performance_midi_input("Midi Through MIDI 0:1"));
    }
}
