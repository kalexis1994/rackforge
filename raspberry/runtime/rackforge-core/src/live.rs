use crate::control::{self, AudioControlCommand};
use crate::session::SessionStore;
use crate::{LoadedPlugin, PluginInstance, PluginPackage};
use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};
use anyhow::{Context, Result, bail};
use midir::{Ignore, MidiInput, MidiInputConnection};
use rackforge_control_api::CONTROL_SOCKET_NAME;
use rackforge_plugin_api::PluginKind;
use rackforge_plugin_api::abi::MidiEventV1;
use rackforge_session_api::{
    DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, InstanceId, PluginInstanceState, Revision,
    SESSION_SCHEMA_VERSION, SessionId, SessionState, SoundSummary,
};
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender};

const OUTPUT_DEVICE: &str = "hw:CARD=USB,DEV=0";
const OUTPUT_RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const PERIOD_FRAMES: usize = 128;
const BUFFER_FRAMES: i64 = 384;
const MAX_EVENTS_PER_BLOCK: usize = 256;
const MIDI_QUEUE_CAPACITY: usize = 2_048;
const AUDIO_CONTROL_QUEUE_CAPACITY: usize = 64;
const MIDI_CHANNELS: usize = 16;
const CONTINUOUS_CONTROLLERS: usize = 120;

struct AuditionLease {
    id: u64,
    instance_id: InstanceId,
    previous_sound_id: Option<String>,
}

struct MidiControllerState {
    continuous_controllers: [[Option<u8>; CONTINUOUS_CONTROLLERS]; MIDI_CHANNELS],
    pitch_bend: [Option<(u8, u8)>; MIDI_CHANNELS],
    channel_pressure: [Option<u8>; MIDI_CHANNELS],
}

impl Default for MidiControllerState {
    fn default() -> Self {
        Self {
            continuous_controllers: [[None; CONTINUOUS_CONTROLLERS]; MIDI_CHANNELS],
            pitch_bend: [None; MIDI_CHANNELS],
            channel_pressure: [None; MIDI_CHANNELS],
        }
    }
}

impl MidiControllerState {
    fn observe(&mut self, event: MidiEventV1) {
        if event.length == 0 {
            return;
        }
        let status = event.data[0] & 0xf0;
        let channel = usize::from(event.data[0] & 0x0f);
        match status {
            0xb0 if event.length >= 3 => {
                let controller = usize::from(event.data[1] & 0x7f);
                if controller < CONTINUOUS_CONTROLLERS {
                    self.continuous_controllers[channel][controller] = Some(event.data[2] & 0x7f);
                } else if controller == 121 {
                    self.continuous_controllers[channel].fill(None);
                    self.pitch_bend[channel] = None;
                    self.channel_pressure[channel] = None;
                }
            }
            0xd0 if event.length >= 2 => {
                self.channel_pressure[channel] = Some(event.data[1] & 0x7f);
            }
            0xe0 if event.length >= 3 => {
                self.pitch_bend[channel] = Some((event.data[1] & 0x7f, event.data[2] & 0x7f));
            }
            _ => {}
        }
    }

    fn replay_into(&self, events: &mut Vec<MidiEventV1>, maximum_events: usize) -> usize {
        let mut omitted = 0;
        for channel in 0..MIDI_CHANNELS {
            for (controller, value) in self.continuous_controllers[channel].iter().enumerate() {
                if let Some(value) = value {
                    push_replay_event(
                        events,
                        maximum_events,
                        MidiEventV1 {
                            frame: 0,
                            length: 3,
                            data: [0xb0 | channel as u8, controller as u8, *value],
                        },
                        &mut omitted,
                    );
                }
            }
            if let Some(pressure) = self.channel_pressure[channel] {
                push_replay_event(
                    events,
                    maximum_events,
                    MidiEventV1 {
                        frame: 0,
                        length: 2,
                        data: [0xd0 | channel as u8, pressure, 0],
                    },
                    &mut omitted,
                );
            }
            if let Some((least_significant, most_significant)) = self.pitch_bend[channel] {
                push_replay_event(
                    events,
                    maximum_events,
                    MidiEventV1 {
                        frame: 0,
                        length: 3,
                        data: [0xe0 | channel as u8, least_significant, most_significant],
                    },
                    &mut omitted,
                );
            }
        }
        omitted
    }
}

fn push_replay_event(
    events: &mut Vec<MidiEventV1>,
    maximum_events: usize,
    event: MidiEventV1,
    omitted: &mut usize,
) {
    if events.len() < maximum_events {
        events.push(event);
    } else {
        *omitted += 1;
    }
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

    let (sender, receiver) = mpsc::sync_channel(MIDI_QUEUE_CAPACITY);
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
    let instance_id =
        InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).map_err(|message| anyhow::anyhow!(message))?;
    let session = SessionState {
        schema_version: SESSION_SCHEMA_VERSION,
        session_id: SessionId::new(DEFAULT_LIVE_SESSION_ID)
            .map_err(|message| anyhow::anyhow!(message))?,
        revision: Revision::ZERO,
        active_instance_id: Some(instance_id.clone()),
        instances: vec![PluginInstanceState {
            instance_id,
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
        }],
        audition: None,
        program_draft: None,
    };
    let session_store = SessionStore::shared(session)?;
    let (control_sender, control_receiver) = mpsc::sync_channel(AUDIO_CONTROL_QUEUE_CAPACITY);
    let control_path = control_socket_path();
    let control_storage = config
        .data_root
        .as_ref()
        .map(|root| crate::PluginStorage::new(root.clone()));
    let _control_server = control::start(
        &control_path,
        session_store,
        control_sender,
        control_storage,
    )?;
    println!("CONTROL_READY socket={}", control_path.display());
    println!("READY_TO_PLAY");
    audio_loop(&pcm, &receiver, &control_receiver, &mut instance)
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
    sender: SyncSender<MidiEventV1>,
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
                    let _ = source_sender.try_send(MidiEventV1 {
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
    control_receiver: &Receiver<AudioControlCommand>,
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
    let mut controller_state = MidiControllerState::default();
    let mut replay_controller_state = false;

    loop {
        while let Ok(command) = control_receiver.try_recv() {
            match command {
                AudioControlCommand::SelectSound {
                    instance_id,
                    sound_id,
                    reply,
                } => {
                    let result = instance.load_preset(&sound_id).map(|()| {
                        println!("LIVE_SOUND_SELECTED instance={instance_id} id={sound_id}");
                    });
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result.map_err(|error| error.to_string()));
                }
                AudioControlCommand::BeginAudition {
                    instance_id,
                    previous_sound_id,
                    reply,
                } => {
                    let result = (|| -> Result<u64, String> {
                        if audition.is_some() {
                            return Err("audition focus is already leased".into());
                        }
                        instance.reset().map_err(|error| error.to_string())?;
                        let lease_id = next_audition_id;
                        next_audition_id = next_audition_id.wrapping_add(1).max(1);
                        audition = Some(AuditionLease {
                            id: lease_id,
                            instance_id: instance_id.clone(),
                            previous_sound_id,
                        });
                        println!("AUDITION_GRANTED lease={lease_id} instance={instance_id}");
                        Ok(lease_id)
                    })();
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result);
                }
                AudioControlCommand::KeepAuditionAlive { lease_id, reply } => {
                    let result = match audition.as_ref() {
                        Some(lease) if lease.id == lease_id => Ok(()),
                        _ => Err("audition lease is missing or no longer valid".into()),
                    };
                    let _ = reply.send(result);
                }
                AudioControlCommand::EndAudition { lease_id, reply } => {
                    let result = match audition.take() {
                        Some(lease) if lease.id == lease_id => {
                            restore_after_audition(instance, &lease)
                                .map_err(|error| error.to_string())
                                .map(|()| {
                                    println!(
                                        "AUDITION_RELEASED lease={lease_id} instance={}",
                                        lease.instance_id
                                    )
                                })
                        }
                        Some(lease) => {
                            audition = Some(lease);
                            Err("audition lease is missing or no longer valid".into())
                        }
                        None => Err("audition lease is missing or no longer valid".into()),
                    };
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result);
                }
                AudioControlCommand::BeginProgramEdit {
                    instance_id,
                    request,
                    previous_sound_id,
                    reply,
                } => {
                    let result = (|| -> Result<_, String> {
                        if audition.is_some() {
                            return Err("audition focus is already leased".into());
                        }
                        let prepared = instance
                            .begin_program_edit(&request)
                            .map_err(|error| error.to_string())?;
                        instance.reset().map_err(|error| error.to_string())?;
                        if !instance
                            .preview_program(&prepared)
                            .map_err(|error| error.to_string())?
                        {
                            instance
                                .load_preset(&prepared.preview_sound_id)
                                .map_err(|error| error.to_string())?;
                        }
                        let lease_id = next_audition_id;
                        next_audition_id = next_audition_id.wrapping_add(1).max(1);
                        audition = Some(AuditionLease {
                            id: lease_id,
                            instance_id: instance_id.clone(),
                            previous_sound_id,
                        });
                        println!(
                            "PROGRAM_EDIT_AUDIO_READY lease={lease_id} instance={instance_id}"
                        );
                        Ok((lease_id, prepared))
                    })();
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result);
                }
                AudioControlCommand::ReplaceProgramDraft {
                    instance_id,
                    document,
                    reply,
                } => {
                    let result = (|| -> Result<_, String> {
                        let prepared = instance
                            .prepare_program_save(&document)
                            .map_err(|error| error.to_string())?;
                        if !instance
                            .preview_program(&prepared)
                            .map_err(|error| error.to_string())?
                        {
                            instance
                                .load_preset(&prepared.preview_sound_id)
                                .map_err(|error| error.to_string())?;
                        }
                        println!("PROGRAM_DRAFT_AUDIO_READY instance={instance_id}");
                        Ok(prepared)
                    })();
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result);
                }
                AudioControlCommand::InstallProgram {
                    instance_id,
                    prepared,
                    reply,
                } => {
                    let result = instance
                        .install_program(&prepared)
                        .and_then(|()| instance.preset_catalog())
                        .map_err(|error| error.to_string())
                        .inspect(|_| {
                            println!(
                                "PROGRAM_INSTALLED instance={instance_id} id={}",
                                prepared.document.id
                            );
                        });
                    let _ = reply.send(result);
                }
                AudioControlCommand::ActivateSurface {
                    instance_id,
                    request,
                    reply,
                } => {
                    let result = instance
                        .activate_surface(&request)
                        .map_err(|error| error.to_string())
                        .inspect(|response| {
                            println!(
                                "SURFACE_ACTIVATED instance={instance_id} mode={:?} focus={:?}",
                                request.mode, response.focus_item_id
                            );
                        });
                    let _ = reply.send(result);
                }
            }
        }
        events.clear();
        if replay_controller_state {
            let omitted = controller_state.replay_into(&mut events, MAX_EVENTS_PER_BLOCK);
            dropped_events += omitted;
            if omitted > 0 {
                eprintln!("MIDI_CONTROLLER_REPLAY_TRUNCATED omitted={omitted}");
            }
            replay_controller_state = false;
        }
        while let Ok(event) = receiver.try_recv() {
            controller_state.observe(event);
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

fn restore_after_audition(instance: &mut PluginInstance<'_>, lease: &AuditionLease) -> Result<()> {
    instance.reset()?;
    if let Some(previous) = &lease.previous_sound_id {
        instance.load_preset(previous)?;
    }
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

    fn midi(length: u8, data: [u8; 3]) -> MidiEventV1 {
        MidiEventV1 {
            frame: 0,
            length,
            data,
        }
    }

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

    #[test]
    fn controller_state_replays_mod_wheel_pitch_and_pressure_by_channel() {
        let mut state = MidiControllerState::default();
        state.observe(midi(3, [0xb2, 1, 87]));
        state.observe(midi(3, [0xe2, 12, 100]));
        state.observe(midi(2, [0xd2, 44, 0]));
        state.observe(midi(3, [0x92, 60, 127]));

        let mut replay = Vec::new();
        assert_eq!(state.replay_into(&mut replay, MAX_EVENTS_PER_BLOCK), 0);
        assert_eq!(replay.len(), 3);
        assert!(replay.iter().any(|event| event.data == [0xb2, 1, 87]));
        assert!(replay.iter().any(|event| event.data == [0xe2, 12, 100]));
        assert!(
            replay
                .iter()
                .any(|event| event.length == 2 && event.data == [0xd2, 44, 0])
        );
    }

    #[test]
    fn reset_all_controllers_clears_the_latched_channel_state() {
        let mut state = MidiControllerState::default();
        state.observe(midi(3, [0xb0, 1, 127]));
        state.observe(midi(3, [0xe0, 0, 127]));
        state.observe(midi(2, [0xd0, 64, 0]));
        state.observe(midi(3, [0xb0, 121, 0]));

        let mut replay = Vec::new();
        assert_eq!(state.replay_into(&mut replay, MAX_EVENTS_PER_BLOCK), 0);
        assert!(replay.is_empty());
    }

    #[test]
    fn controller_replay_is_bounded_without_forgetting_state() {
        let mut state = MidiControllerState::default();
        state.observe(midi(3, [0xb0, 1, 10]));
        state.observe(midi(3, [0xb0, 11, 20]));

        let mut first = Vec::new();
        assert_eq!(state.replay_into(&mut first, 1), 1);
        assert_eq!(first.len(), 1);

        let mut second = Vec::new();
        assert_eq!(state.replay_into(&mut second, 2), 0);
        assert_eq!(second.len(), 2);
    }
}
