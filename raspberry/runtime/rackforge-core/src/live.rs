use crate::audio::{OpenedAudioOutput, discover_audio_devices, open_audio_output_from_inventory};
use crate::control::{self, AudioControlCommand};
use crate::session::SessionStore;
use crate::session_checkpoint::SessionCheckpointStore;
use crate::{LoadedPlugin, PluginInstance, PluginPackage};
use alsa::pcm::PCM;
use anyhow::{Context, Result, bail};
use midir::{Ignore, MidiInput, MidiInputConnection};
use rackforge_audio_api::{
    AUDIO_OUTPUT_STATE_SCHEMA_VERSION, AudioOutputProfile, AudioOutputState, AudioSampleFormat,
};
use rackforge_control_api::CONTROL_SOCKET_NAME;
use rackforge_midi_api::{
    CompiledMidiRoute, DEFAULT_INPUT_BUS_ID, IngressMidiEvent, MIDI_ROUTING_SCHEMA_VERSION,
    MidiInputBusId, MidiPacket, MidiRoute, MidiRouteId, MidiRouteMatch, MidiRouteTarget,
    MidiRouteTransform, MidiSourceDescriptor, MidiSourceId, MidiSourceKey, MidiSourceRegistry,
    MidiTargetId, PluginChannelModel,
};
use rackforge_plugin_api::PluginKind;
use rackforge_plugin_api::abi::MidiEventV1;
use rackforge_session_api::{
    DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, HostControlBinding, InstanceId, MasterLevel,
    MasterPan, PluginInstanceState, Revision, SESSION_SCHEMA_VERSION, SessionId, SessionState,
    SoundSummary, SurfaceMode,
};
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

const MAX_EVENTS_PER_BLOCK: usize = 256;
const MIDI_QUEUE_CAPACITY: usize = 2_048;
const AUDIO_CONTROL_QUEUE_CAPACITY: usize = 64;
const MIDI_CHANNELS: usize = 16;
const CONTINUOUS_CONTROLLERS: usize = 120;
const MASTER_LEVEL_SMOOTHING_FRAMES: u32 = 480;

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
        self.remaining = MASTER_LEVEL_SMOOTHING_FRAMES;
        self.step = (self.target - self.current) / self.remaining as f32;
    }

    fn next_gain(&mut self) -> f32 {
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
        self.remaining = MASTER_LEVEL_SMOOTHING_FRAMES;
        self.step_left = (self.target_left - self.current_left) / self.remaining as f32;
        self.step_right = (self.target_right - self.current_right) / self.remaining as f32;
    }

    fn next_balance(&mut self) -> (f32, f32) {
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

struct ReservedMidiControls {
    control_changes: [[bool; 120]; MIDI_CHANNELS],
}

impl Default for ReservedMidiControls {
    fn default() -> Self {
        Self {
            control_changes: [[false; 120]; MIDI_CHANNELS],
        }
    }
}

impl ReservedMidiControls {
    fn replace(&mut self, bindings: &[HostControlBinding]) {
        self.control_changes = [[false; CONTINUOUS_CONTROLLERS]; MIDI_CHANNELS];
        for binding in bindings {
            self.control_changes[binding.midi_cc.channel as usize]
                [binding.midi_cc.controller as usize] = true;
        }
    }

    fn contains(&self, event: MidiEventV1) -> bool {
        if event.length != 3 || event.data[0] & 0xf0 != 0xb0 || event.data[1] > 119 {
            return false;
        }
        self.control_changes[(event.data[0] & 0x0f) as usize][event.data[1] as usize]
    }
}

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

struct MidiControllerStates {
    sources: Vec<MidiControllerState>,
}

impl MidiControllerStates {
    fn new(source_count: usize) -> Self {
        Self {
            sources: (0..source_count)
                .map(|_| MidiControllerState::default())
                .collect(),
        }
    }

    fn observe(&mut self, source: MidiSourceKey, event: MidiEventV1) {
        if let Some(state) = self.sources.get_mut(source.get() as usize) {
            state.observe(event);
        }
    }

    fn replay_routed_into(
        &self,
        route: &CompiledMidiRoute,
        events: &mut Vec<MidiEventV1>,
        maximum_events: usize,
    ) -> usize {
        let mut omitted = 0;
        for (source_index, state) in self.sources.iter().enumerate() {
            state.visit_replay(|event| {
                let ingress = IngressMidiEvent {
                    source: MidiSourceKey::new(source_index as u32),
                    packet: MidiPacket {
                        frame: event.frame,
                        length: event.length,
                        data: event.data,
                    },
                };
                if let Some(routed) = route.route(ingress) {
                    push_replay_event(
                        events,
                        maximum_events,
                        plugin_midi_event(routed.packet),
                        &mut omitted,
                    );
                }
            });
        }
        omitted
    }
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

    fn visit_replay(&self, mut visit: impl FnMut(MidiEventV1)) {
        for channel in 0..MIDI_CHANNELS {
            for (controller, value) in self.continuous_controllers[channel].iter().enumerate() {
                if let Some(value) = value {
                    visit(MidiEventV1 {
                        frame: 0,
                        length: 3,
                        data: [0xb0 | channel as u8, controller as u8, *value],
                    });
                }
            }
            if let Some(pressure) = self.channel_pressure[channel] {
                visit(MidiEventV1 {
                    frame: 0,
                    length: 2,
                    data: [0xd0 | channel as u8, pressure, 0],
                });
            }
            if let Some((least_significant, most_significant)) = self.pitch_bend[channel] {
                visit(MidiEventV1 {
                    frame: 0,
                    length: 3,
                    data: [0xe0 | channel as u8, least_significant, most_significant],
                });
            }
        }
    }

    #[cfg(test)]
    fn replay_into(&self, events: &mut Vec<MidiEventV1>, maximum_events: usize) -> usize {
        let mut omitted = 0;
        self.visit_replay(|event| {
            push_replay_event(events, maximum_events, event, &mut omitted);
        });
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
    pub audio_output: AudioOutputProfile,
    pub audio_state_path: PathBuf,
}

pub fn run(config: LiveConfig) -> Result<()> {
    ensure_supported_engine_profile(&config.audio_output)?;
    let output_rate = config.audio_output.sample_rate_hz;
    let period_frames = config.audio_output.period_frames as usize;
    let channels = config.audio_output.channels as usize;
    let session_id =
        SessionId::new(DEFAULT_LIVE_SESSION_ID).map_err(|message| anyhow::anyhow!(message))?;
    let checkpoint = config
        .data_root
        .as_deref()
        .map(SessionCheckpointStore::live);
    let (persisted_mode, persisted_sound_id, persisted_master_level, persisted_master_pan) =
        match checkpoint.as_ref() {
            Some(store) => match (
                store.active_mode(&session_id),
                store.selected_sound(&session_id, DEFAULT_LIVE_INSTANCE_ID),
                store.master_level(&session_id),
                store.master_pan(&session_id),
            ) {
                (Ok(mode), Ok(sound_id), Ok(master_level), Ok(master_pan)) => {
                    (mode, sound_id, master_level, master_pan)
                }
                (mode, sound_id, master_level, master_pan) => {
                    if let Err(error) = mode {
                        eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                    }
                    if let Err(error) = sound_id {
                        eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                    }
                    if let Err(error) = master_level {
                        eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                    }
                    if let Err(error) = master_pan {
                        eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                    }
                    (None, None, None, None)
                }
            },
            None => (None, None, None, None),
        };
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
    let configured_preset = match config.preset.as_deref() {
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
    let preset = match persisted_sound_id.as_deref() {
        Some(id) => match presets
            .presets
            .iter()
            .chain(plugin.presets().presets.iter())
            .find(|preset| preset.id == id)
        {
            Some(preset) => Some(preset),
            None => {
                eprintln!("SESSION_CHECKPOINT_SOUND_MISSING id={id:?}; using configured fallback");
                configured_preset
            }
        },
        None => configured_preset,
    };
    if let Some(preset) = preset {
        instance.load_preset(&preset.id)?;
        println!("LIVE_PRESET_READY id={} name={:?}", preset.id, preset.name);
    }
    instance.activate(
        f64::from(output_rate),
        period_frames as u32,
        0,
        channels as u32,
    )?;

    let (sender, receiver) = mpsc::sync_channel(MIDI_QUEUE_CAPACITY);
    let (_midi_connections, midi_port_names, midi_sources) = connect_midi_sources(sender)?;
    println!("MIDI_READY ports={midi_port_names:?}");
    let play_route = compile_default_play_route(
        &midi_sources,
        plugin
            .manifest()
            .midi
            .as_ref()
            .map(|midi| midi.channel_model)
            .unwrap_or(PluginChannelModel::SinglePart),
    )?;
    println!(
        "MIDI_ROUTE_READY id={} source=primary input=omni output=auto target={}.{}",
        play_route_id(),
        DEFAULT_LIVE_INSTANCE_ID,
        DEFAULT_INPUT_BUS_ID
    );
    let audio_devices = discover_audio_devices()?;
    let output = open_audio_output_from_inventory(&config.audio_output, &audio_devices)?;
    println!(
        "AUDIO_READY id={} name={:?} backend={} rate={} channels={} format={:?} \
         period={} buffer={} nominal_buffer_ms={:.2}",
        output.device.id,
        output.device.name,
        output.device.backend_address,
        output.profile.sample_rate_hz,
        output.profile.channels,
        output.profile.sample_format,
        output.profile.period_frames,
        output.profile.buffer_frames,
        output.profile.nominal_buffer_latency_ms(),
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
        session_id,
        revision: Revision::ZERO,
        active_mode: persisted_mode.unwrap_or(SurfaceMode::Live),
        master_level: persisted_master_level.unwrap_or(MasterLevel::UNITY),
        master_pan: persisted_master_pan.unwrap_or(MasterPan::CENTER),
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
    if let Some(checkpoint) = &checkpoint {
        checkpoint
            .save(&session)
            .context("saving initial LIVE session checkpoint")?;
    }
    let initial_master_level = session.master_level;
    let initial_master_pan = session.master_pan;
    let session_store = SessionStore::shared(session)?;
    let audio_state = Arc::new(Mutex::new(AudioOutputState {
        schema_version: AUDIO_OUTPUT_STATE_SCHEMA_VERSION,
        active_device: output.device.clone(),
        active_profile: output.profile.clone(),
        devices: audio_devices,
    }));
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
        Arc::clone(&audio_state),
        config.audio_state_path,
        control_storage,
        checkpoint,
    )?;
    println!("CONTROL_READY socket={}", control_path.display());
    println!("READY_TO_PLAY");
    audio_loop(
        output,
        &receiver,
        &control_receiver,
        &mut instance,
        &play_route,
        midi_port_names.len(),
        initial_master_level,
        initial_master_pan,
        audio_state,
    )
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
    sender: SyncSender<IngressMidiEvent>,
) -> Result<(
    Vec<MidiInputConnection<()>>,
    Vec<String>,
    MidiSourceRegistry,
)> {
    let discovery = MidiInput::new("rackforge-core-discovery")?;
    let names = performance_midi_names(&discovery)?;
    let mut connections = Vec::with_capacity(names.len());
    let mut registry = MidiSourceRegistry::default();
    for (index, name) in names.iter().enumerate() {
        let source = MidiSourceKey::new(index as u32);
        registry.register(
            source,
            MidiSourceDescriptor {
                id: stable_alsa_source_id(name)?,
                name: name.clone(),
                primary: index == 0,
            },
        )?;
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
                    if let Ok(packet) = MidiPacket::new(0, message) {
                        let _ = source_sender.try_send(IngressMidiEvent { source, packet });
                    }
                },
                (),
            )
            .map_err(|error| anyhow::anyhow!("connecting MIDI input {name:?}: {error}"))?;
        connections.push(connection);
    }
    Ok((connections, names, registry))
}

fn stable_alsa_source_id(name: &str) -> Result<MidiSourceId> {
    let stable_name = name
        .rsplit_once(' ')
        .filter(|(_, suffix)| {
            suffix.split_once(':').is_some_and(|(client, port)| {
                !client.is_empty()
                    && !port.is_empty()
                    && client.bytes().all(|byte| byte.is_ascii_digit())
                    && port.bytes().all(|byte| byte.is_ascii_digit())
            })
        })
        .map_or(name, |(stable, _)| stable);
    let mut slug = String::with_capacity(stable_name.len());
    let mut separator = false;
    for byte in stable_name.bytes() {
        if byte.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(char::from(byte.to_ascii_lowercase()));
            separator = false;
        } else {
            separator = true;
        }
    }
    if slug.is_empty() {
        slug.push_str("unnamed");
    }
    let hash = stable_name
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    MidiSourceId::new(format!("alsa.{slug}-{hash:016x}")).map_err(Into::into)
}

fn play_route_id() -> &'static str {
    "play.primary"
}

fn compile_default_play_route(
    sources: &MidiSourceRegistry,
    channel_model: PluginChannelModel,
) -> Result<CompiledMidiRoute> {
    let route = MidiRoute {
        schema_version: MIDI_ROUTING_SCHEMA_VERSION,
        id: MidiRouteId::new(play_route_id())?,
        enabled: true,
        matches: MidiRouteMatch::default(),
        transform: MidiRouteTransform::default(),
        target: MidiRouteTarget {
            instance_id: MidiTargetId::new(DEFAULT_LIVE_INSTANCE_ID)?,
            input_bus_id: MidiInputBusId::new(DEFAULT_INPUT_BUS_ID)?,
        },
    };
    route.compile(sources, channel_model).map_err(Into::into)
}

fn ensure_supported_engine_profile(profile: &AudioOutputProfile) -> Result<()> {
    profile.validate()?;
    if profile.sample_format != AudioSampleFormat::S32Le {
        bail!(
            "audio engine currently renders S32_LE only, requested {:?}",
            profile.sample_format
        );
    }
    if profile.channels != 2 {
        bail!(
            "audio engine currently renders stereo only, requested {} channels",
            profile.channels
        );
    }
    Ok(())
}

fn audio_loop(
    initial_output: OpenedAudioOutput,
    receiver: &Receiver<IngressMidiEvent>,
    control_receiver: &Receiver<AudioControlCommand>,
    instance: &mut PluginInstance<'_>,
    play_route: &CompiledMidiRoute,
    midi_source_count: usize,
    initial_master_level: MasterLevel,
    initial_master_pan: MasterPan,
    audio_state: Arc<Mutex<AudioOutputState>>,
) -> Result<()> {
    let mut output = Some(initial_output);
    let mut period_frames = output.as_ref().unwrap().profile.period_frames as usize;
    let mut channels = output.as_ref().unwrap().profile.channels as usize;
    let mut output_rate = output.as_ref().unwrap().profile.sample_rate_hz as usize;
    let input = Vec::new();
    let mut plugin_output = vec![0.0_f32; period_frames * channels];
    let mut device_output = vec![0_i32; period_frames * channels];
    let mut events = Vec::with_capacity(MAX_EVENTS_PER_BLOCK);
    let mut meter_frames = 0_usize;
    let mut meter_peak = 0_f32;
    let mut meter_clipped = 0_usize;
    let mut dropped_events = 0_usize;
    let mut audition: Option<AuditionLease> = None;
    let mut next_audition_id = 1_u64;
    let mut controller_states = MidiControllerStates::new(midi_source_count);
    let mut replay_controller_state = false;
    let mut master_gain = MasterGain::new(initial_master_level);
    let mut master_balance = MasterBalance::new(initial_master_pan);
    let mut reserved_midi_controls = ReservedMidiControls::default();

    loop {
        while let Ok(command) = control_receiver.try_recv() {
            match command {
                AudioControlCommand::ApplyAudioOutput { profile, reply } => {
                    let result =
                        reconfigure_audio_output(&mut output, instance, profile, &audio_state);
                    if let Ok(snapshot) = &result {
                        period_frames = snapshot.active_profile.period_frames as usize;
                        channels = snapshot.active_profile.channels as usize;
                        output_rate = snapshot.active_profile.sample_rate_hz as usize;
                        plugin_output.resize(period_frames * channels, 0.0);
                        device_output.resize(period_frames * channels, 0);
                        meter_frames = 0;
                        meter_peak = 0.0;
                        meter_clipped = 0;
                    }
                    let _ = reply.send(result.map_err(|error| error.to_string()));
                }
                AudioControlCommand::RegisterHostControls {
                    controller_id,
                    bindings,
                    reply,
                } => {
                    reserved_midi_controls.replace(&bindings);
                    println!(
                        "HOST_CONTROLS_REGISTERED controller={controller_id} count={}",
                        bindings.len()
                    );
                    let _ = reply.send(Ok(()));
                }
                AudioControlCommand::SetMasterLevel { level, reply } => {
                    master_gain.set_level(level);
                    let _ = reply.send(Ok(()));
                }
                AudioControlCommand::SetMasterPan { pan, reply } => {
                    master_balance.set_pan(pan);
                    let _ = reply.send(Ok(()));
                }
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
                        let editor = instance
                            .program_editor_view(&prepared.document)
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
                        Ok((lease_id, prepared, editor))
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
                        let editor = instance
                            .program_editor_view(&prepared.document)
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
                        Ok((prepared, editor))
                    })();
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result);
                }
                AudioControlCommand::EditProgramDraftField {
                    instance_id,
                    request,
                    reply,
                } => {
                    let result = (|| -> Result<_, String> {
                        let prepared = instance
                            .apply_program_edit(&request)
                            .map_err(|error| error.to_string())?;
                        let editor = instance
                            .program_editor_view(&prepared.document)
                            .map_err(|error| error.to_string())?;
                        if !instance
                            .preview_program(&prepared)
                            .map_err(|error| error.to_string())?
                        {
                            instance
                                .load_preset(&prepared.preview_sound_id)
                                .map_err(|error| error.to_string())?;
                        }
                        println!(
                            "PROGRAM_FIELD_AUDIO_READY instance={instance_id} field={}",
                            request.field_id
                        );
                        Ok((prepared, editor))
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
            let omitted =
                controller_states.replay_routed_into(play_route, &mut events, MAX_EVENTS_PER_BLOCK);
            dropped_events += omitted;
            if omitted > 0 {
                eprintln!("MIDI_CONTROLLER_REPLAY_TRUNCATED omitted={omitted}");
            }
            replay_controller_state = false;
        }
        while let Ok(event) = receiver.try_recv() {
            let plugin_event = plugin_midi_event(event.packet);
            if reserved_midi_controls.contains(plugin_event) {
                continue;
            }
            controller_states.observe(event.source, plugin_event);
            if let Some(routed) = play_route.route(event) {
                if events.len() < MAX_EVENTS_PER_BLOCK {
                    events.push(plugin_midi_event(routed.packet));
                } else {
                    dropped_events += 1;
                }
            }
        }

        plugin_output.fill(0.0);
        instance.process_interleaved(
            &input,
            &mut plugin_output,
            period_frames as u32,
            0,
            channels as u32,
            &events,
            &[],
        )?;
        for (source_frame, target_frame) in plugin_output
            .chunks_exact(channels)
            .zip(device_output.chunks_exact_mut(channels))
        {
            let gain = master_gain.next_gain();
            let (left_balance, right_balance) = master_balance.next_balance();
            for (channel, (sample, target)) in source_frame.iter().zip(target_frame).enumerate() {
                let balance = if channel == 0 {
                    left_balance
                } else {
                    right_balance
                };
                let mastered = sample * gain * balance;
                meter_peak = meter_peak.max(mastered.abs());
                meter_clipped += usize::from(mastered.abs() > 0.95);
                *target = (mastered.clamp(-0.95, 0.95) * i32::MAX as f32) as i32;
            }
        }
        meter_frames += period_frames;
        let current_output = output
            .as_ref()
            .context("audio output disappeared after reconfiguration")?;
        let io = current_output.pcm.io_i32()?;
        write_period(
            &current_output.pcm,
            &io,
            &device_output,
            period_frames,
            channels,
        )?;

        if meter_frames >= output_rate {
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

fn reconfigure_audio_output(
    output: &mut Option<OpenedAudioOutput>,
    instance: &mut PluginInstance<'_>,
    requested: AudioOutputProfile,
    shared_state: &Arc<Mutex<AudioOutputState>>,
) -> Result<AudioOutputState> {
    ensure_supported_engine_profile(&requested)?;
    let previous_profile = output
        .as_ref()
        .context("audio output is unavailable")?
        .profile
        .clone();
    if requested == previous_profile {
        return shared_state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| anyhow::anyhow!("audio state lock is poisoned"));
    }

    instance
        .deactivate()
        .context("deactivating plugin for audio change")?;
    drop(output.take());
    let apply = (|| -> Result<(OpenedAudioOutput, Vec<_>)> {
        let devices = discover_audio_devices().context("refreshing audio device inventory")?;
        let opened = open_audio_output_from_inventory(&requested, &devices)?;
        instance
            .activate(
                f64::from(requested.sample_rate_hz),
                requested.period_frames,
                0,
                requested.channels,
            )
            .context("activating plugin for requested audio profile")?;
        Ok((opened, devices))
    })();

    match apply {
        Ok((opened, devices)) => {
            let state = AudioOutputState {
                schema_version: AUDIO_OUTPUT_STATE_SCHEMA_VERSION,
                active_device: opened.device.clone(),
                active_profile: opened.profile.clone(),
                devices,
            };
            state.validate().context("validating applied audio state")?;
            *output = Some(opened);
            *shared_state
                .lock()
                .map_err(|_| anyhow::anyhow!("audio state lock is poisoned"))? = state.clone();
            println!(
                "AUDIO_RECONFIGURED id={} rate={} period={} buffer={} nominal_buffer_ms={:.2}",
                state.active_device.id,
                state.active_profile.sample_rate_hz,
                state.active_profile.period_frames,
                state.active_profile.buffer_frames,
                state.active_profile.nominal_buffer_latency_ms(),
            );
            Ok(state)
        }
        Err(apply_error) => {
            let rollback = (|| -> Result<OpenedAudioOutput> {
                let devices = discover_audio_devices()?;
                let opened = open_audio_output_from_inventory(&previous_profile, &devices)?;
                instance.activate(
                    f64::from(previous_profile.sample_rate_hz),
                    previous_profile.period_frames,
                    0,
                    previous_profile.channels,
                )?;
                Ok(opened)
            })();
            match rollback {
                Ok(opened) => {
                    *output = Some(opened);
                    bail!("audio change rejected: {apply_error:#}; previous output restored")
                }
                Err(rollback_error) => bail!(
                    "audio change failed: {apply_error:#}; rollback failed: {rollback_error:#}"
                ),
            }
        }
    }
}

fn plugin_midi_event(packet: MidiPacket) -> MidiEventV1 {
    MidiEventV1 {
        frame: packet.frame,
        length: packet.length,
        data: packet.data,
    }
}

fn restore_after_audition(instance: &mut PluginInstance<'_>, lease: &AuditionLease) -> Result<()> {
    instance.reset()?;
    if let Some(previous) = &lease.previous_sound_id {
        instance.load_preset(previous)?;
    }
    Ok(())
}

fn write_period(
    pcm: &PCM,
    io: &alsa::pcm::IO<'_, i32>,
    output: &[i32],
    period_frames: usize,
    channels: usize,
) -> Result<()> {
    let mut frame_offset = 0;
    while frame_offset < period_frames {
        match io.writei(&output[frame_offset * channels..]) {
            Ok(0) => bail!("audio output accepted zero frames"),
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
    use rackforge_session_api::{HostControlTarget, MidiControlChangeBinding};

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

    #[test]
    fn controller_state_is_kept_separately_for_each_midi_source() {
        let mut states = MidiControllerStates::new(2);
        states.observe(MidiSourceKey::new(0), midi(3, [0xb0, 1, 20]));
        states.observe(MidiSourceKey::new(1), midi(3, [0xb0, 1, 100]));

        let mut first = Vec::new();
        assert_eq!(
            states.sources[0].replay_into(&mut first, MAX_EVENTS_PER_BLOCK),
            0
        );
        assert_eq!(first[0].data, [0xb0, 1, 20]);

        let mut second = Vec::new();
        assert_eq!(
            states.sources[1].replay_into(&mut second, MAX_EVENTS_PER_BLOCK),
            0
        );
        assert_eq!(second[0].data, [0xb0, 1, 100]);
    }

    #[test]
    fn stable_alsa_source_identity_ignores_the_ephemeral_client_port() {
        let first =
            stable_alsa_source_id("KL Essential 61 mk3:KL Essential 61 mk3 MIDI 24:0").unwrap();
        let second =
            stable_alsa_source_id("KL Essential 61 mk3:KL Essential 61 mk3 MIDI 28:0").unwrap();
        assert_eq!(first, second);
        assert!(first.as_str().starts_with("alsa.kl-essential-61-mk3"));
    }

    #[test]
    fn default_play_route_uses_only_primary_and_normalizes_single_part_channel() {
        let mut sources = MidiSourceRegistry::default();
        sources
            .register(
                MidiSourceKey::new(0),
                MidiSourceDescriptor {
                    id: MidiSourceId::new("controller.primary").unwrap(),
                    name: "Primary".into(),
                    primary: true,
                },
            )
            .unwrap();
        sources
            .register(
                MidiSourceKey::new(1),
                MidiSourceDescriptor {
                    id: MidiSourceId::new("controller.secondary").unwrap(),
                    name: "Secondary".into(),
                    primary: false,
                },
            )
            .unwrap();
        let route = compile_default_play_route(&sources, PluginChannelModel::SinglePart).unwrap();
        let primary = IngressMidiEvent {
            source: MidiSourceKey::new(0),
            packet: MidiPacket::new(0, &[0x97, 60, 100]).unwrap(),
        };
        let secondary = IngressMidiEvent {
            source: MidiSourceKey::new(1),
            packet: MidiPacket::new(0, &[0x97, 60, 100]).unwrap(),
        };

        assert_eq!(route.route(primary).unwrap().packet.data, [0x90, 60, 100]);
        assert!(route.route(secondary).is_none());
    }

    #[test]
    fn master_gain_is_tapered_and_reaches_new_level_without_a_step() {
        let half = MasterLevel::new(500).unwrap();
        assert!((half.amplitude() - 0.25).abs() < f32::EPSILON);
        let mut gain = MasterGain::new(MasterLevel::UNITY);
        gain.set_level(MasterLevel::SILENT);

        let first = gain.next_gain();
        assert!(first < 1.0 && first > 0.0);
        for _ in 1..MASTER_LEVEL_SMOOTHING_FRAMES {
            gain.next_gain();
        }
        assert_eq!(gain.current, 0.0);
        assert_eq!(gain.remaining, 0);
    }

    #[test]
    fn master_balance_is_neutral_at_center_and_smoothed_to_the_side() {
        let mut balance = MasterBalance::new(MasterPan::CENTER);
        assert_eq!(balance.next_balance(), (1.0, 1.0));
        balance.set_pan(MasterPan::LEFT);

        let first = balance.next_balance();
        assert_eq!(first.0, 1.0);
        assert!(first.1 < 1.0 && first.1 > 0.0);
        for _ in 1..MASTER_LEVEL_SMOOTHING_FRAMES {
            balance.next_balance();
        }
        assert_eq!(balance.current_left, 1.0);
        assert_eq!(balance.current_right, 0.0);
        assert_eq!(balance.remaining, 0);
    }

    #[test]
    fn reserved_host_control_never_reaches_plugin_midi() {
        let mut reserved = ReservedMidiControls::default();
        reserved.replace(&[HostControlBinding {
            target: HostControlTarget::MasterLevel,
            midi_cc: MidiControlChangeBinding {
                channel: 0,
                controller: 82,
            },
        }]);

        assert!(reserved.contains(midi(3, [0xb0, 82, 64])));
        assert!(!reserved.contains(midi(3, [0xb0, 83, 64])));
        assert!(!reserved.contains(midi(3, [0xb1, 82, 64])));
        assert!(!reserved.contains(midi(3, [0x90, 82, 64])));

        reserved.replace(&[]);
        assert!(!reserved.contains(midi(3, [0xb0, 82, 64])));
    }

    #[test]
    fn engine_profile_rejects_formats_and_layouts_not_rendered_yet() {
        let mut profile = AudioOutputProfile {
            device: rackforge_audio_api::AudioDeviceSelector::Usb {
                vendor_id: 0x1235,
                product_id: 0x8211,
                serial: None,
            },
            fallback: rackforge_audio_api::AudioFallbackPolicy::None,
            sample_format: AudioSampleFormat::S32Le,
            sample_rate_hz: 48_000,
            channels: 2,
            period_frames: 128,
            buffer_frames: 384,
        };
        ensure_supported_engine_profile(&profile).unwrap();
        profile.sample_format = AudioSampleFormat::S16Le;
        assert!(ensure_supported_engine_profile(&profile).is_err());
        profile.sample_format = AudioSampleFormat::S32Le;
        profile.channels = 1;
        assert!(ensure_supported_engine_profile(&profile).is_err());
    }
}
