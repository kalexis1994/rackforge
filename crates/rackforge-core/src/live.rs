use crate::audio::{OpenedAudioOutput, discover_audio_devices, open_audio_output_from_inventory};
use crate::control::{self, AudioControlCommand, RackSlotRuntimeSpec, RackSlotStateLoad};
use crate::midi_hotplug::{
    self, SupervisedSource, is_performance_midi_input, stable_alsa_source_id,
};
use crate::performance::{PerformanceBootstrap, PerformanceRepository};
use crate::realtime::{self, XrunMonitor};
use crate::session::SessionStore;
use crate::session_checkpoint::SessionCheckpointStore;
use crate::{LoadedPlugin, PluginInstance, PluginPackage, PluginStateStore};
use alsa::pcm::PCM;
use anyhow::{Context, Result, bail};
use midir::MidiInput;
use rackforge_audio_api::{
    AUDIO_OUTPUT_STATE_SCHEMA_VERSION, AudioOutputProfile, AudioOutputState, AudioSampleFormat,
};
use rackforge_control_api::{CONTROL_SOCKET_NAME, PluginParameterValue};
use rackforge_midi_api::{
    CompiledMidiRoute, DEFAULT_INPUT_BUS_ID, IngressMidiEvent, MIDI_ROUTING_SCHEMA_VERSION,
    MidiInputBusId, MidiPacket, MidiRoute, MidiRouteId, MidiRouteMatch, MidiRouteTarget,
    MidiRouteTransform, MidiSourceDescriptor, MidiSourceKey, MidiSourceRegistry, MidiTargetId,
    PluginChannelModel,
};
use rackforge_performance_api::RackKeyboardParts;
use rackforge_plugin_api::abi::MidiEventV1;
use rackforge_plugin_api::{ParameterKind, PluginKind};
use rackforge_session_api::{
    ButtonPhase, DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, HostActionBinding,
    HostActionTarget, HostControlBinding, InstanceId, MasterLevel, MasterPan, MidiButtonBinding,
    PluginInstanceState, Revision, SESSION_SCHEMA_VERSION, SessionId, SessionState, SoundSummary,
    SurfaceMode,
};
use semver::Version;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::{env, fs};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AudioRenderMode {
    Silent,
    Rack,
    Plugin,
}

impl From<SurfaceMode> for AudioRenderMode {
    fn from(mode: SurfaceMode) -> Self {
        match mode {
            SurfaceMode::Idle => Self::Silent,
            SurfaceMode::Live => Self::Rack,
            SurfaceMode::Play => Self::Plugin,
        }
    }
}

struct RackSlotVoice<'plugin> {
    slot_id: String,
    plugin: &'plugin LoadedPlugin,
    instance: PluginInstance<'plugin>,
    midi_input_channel: Option<u8>,
    midi_note_low: u8,
    midi_note_high: u8,
    midi_transpose: i8,
    keyboard_parts: Option<RackKeyboardParts>,
    level: f32,
    pan: f32,
    output: Vec<f32>,
    events: Vec<MidiEventV1>,
    process_faulted: bool,
}

struct StandaloneVoice<'plugin> {
    instance_id: InstanceId,
    plugin: &'plugin LoadedPlugin,
    instance: PluginInstance<'plugin>,
}

fn create_rack_voices<'plugin>(
    plugins: &BTreeMap<String, &'plugin LoadedPlugin>,
    specs: &[RackSlotRuntimeSpec],
    sample_rate_hz: u32,
    period_frames: u32,
    channels: u32,
) -> Result<Vec<RackSlotVoice<'plugin>>> {
    let mut voices = Vec::with_capacity(specs.len());
    for spec in specs {
        let plugin = plugins
            .get(&spec.plugin_id)
            .with_context(|| format!("plugin {} is not loaded", spec.plugin_id))?;
        let mut instance = plugin.create_instance()?;
        match &spec.state {
            RackSlotStateLoad::Default => {}
            RackSlotStateLoad::Opaque(bytes) => instance
                .load_state(bytes)
                .with_context(|| format!("restoring Rack Slot {} state", spec.slot_id))?,
            RackSlotStateLoad::LegacyPreset(preset_id) => {
                instance.load_preset(preset_id).with_context(|| {
                    format!(
                        "loading legacy program {:?} for Rack Slot {}",
                        preset_id, spec.slot_id
                    )
                })?
            }
        }
        instance
            .activate(f64::from(sample_rate_hz), period_frames, 0, channels)
            .with_context(|| format!("activating Rack Slot {}", spec.slot_id))?;
        voices.push(RackSlotVoice {
            slot_id: spec.slot_id.clone(),
            plugin,
            instance,
            midi_input_channel: spec.midi_input_channel,
            midi_note_low: spec.midi_note_low,
            midi_note_high: spec.midi_note_high,
            midi_transpose: spec.midi_transpose,
            keyboard_parts: spec.keyboard_parts,
            level: f32::from(spec.level_per_mille) / 1_000.0,
            pan: f32::from(spec.pan_per_mille) / 1_000.0,
            output: vec![0.0; period_frames as usize * channels as usize],
            events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            process_faulted: false,
        });
    }
    Ok(voices)
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
    keyboard_parts: Option<MidiButtonBinding>,
    sources: Vec<ReservedMidiSourceState>,
}

struct ReservedMidiSourceState {
    keyboard_parts_held: bool,
    suppressed_notes: [[bool; 128]; MIDI_CHANNELS],
}

impl Default for ReservedMidiControls {
    fn default() -> Self {
        Self::with_sources(1)
    }
}

impl ReservedMidiControls {
    fn with_sources(source_count: usize) -> Self {
        Self {
            control_changes: [[false; 120]; MIDI_CHANNELS],
            keyboard_parts: None,
            sources: (0..source_count)
                .map(|_| ReservedMidiSourceState {
                    keyboard_parts_held: false,
                    suppressed_notes: [[false; 128]; MIDI_CHANNELS],
                })
                .collect(),
        }
    }

    fn replace(&mut self, controls: &[HostControlBinding], actions: &[HostActionBinding]) {
        self.control_changes = [[false; CONTINUOUS_CONTROLLERS]; MIDI_CHANNELS];
        self.keyboard_parts = None;
        for source in &mut self.sources {
            source.keyboard_parts_held = false;
            source.suppressed_notes = [[false; 128]; MIDI_CHANNELS];
        }
        for binding in controls {
            self.control_changes[binding.midi_cc.channel as usize]
                [binding.midi_cc.controller as usize] = true;
        }
        for binding in actions {
            self.control_changes[binding.midi_cc.channel as usize]
                [binding.midi_cc.controller as usize] = true;
            if binding.target == HostActionTarget::KeyboardParts {
                self.keyboard_parts = Some(binding.midi_cc);
            }
        }
    }

    fn consume(&mut self, source: MidiSourceKey, event: MidiEventV1) -> bool {
        let message = &event.data[..usize::from(event.length.min(3))];
        if let Some(binding) = self.keyboard_parts
            && let Some(phase) = binding.phase(message)
        {
            if let Some(state) = self.sources.get_mut(source.get() as usize) {
                state.keyboard_parts_held = phase == ButtonPhase::Press;
            }
            return true;
        }
        if event.length == 3 && event.data[0] & 0xf0 == 0xb0 && event.data[1] <= 119 {
            if self.control_changes[(event.data[0] & 0x0f) as usize][event.data[1] as usize] {
                return true;
            }
        }
        let Some(state) = self.sources.get_mut(source.get() as usize) else {
            return false;
        };
        if event.length < 2 {
            return false;
        }
        let channel = (event.data[0] & 0x0f) as usize;
        let note = event.data[1] as usize;
        let status = event.data[0] & 0xf0;
        if status == 0x90 && event.length == 3 && event.data[2] > 0 && state.keyboard_parts_held {
            state.suppressed_notes[channel][note] = true;
            return true;
        }
        let release = status == 0x80 || (status == 0x90 && event.length == 3 && event.data[2] == 0);
        if release && state.suppressed_notes[channel][note] {
            state.suppressed_notes[channel][note] = false;
            return true;
        }
        if status == 0xa0 && state.suppressed_notes[channel][note] {
            return true;
        }
        false
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
        input_channel: Option<u8>,
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
                if !matches_midi_input_channel(ingress.packet, input_channel) {
                    return;
                }
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

fn discover_plugin_packages(primary: &Path) -> Result<Vec<PluginPackage>> {
    let primary = PluginPackage::open(primary)?;
    let primary_id = primary.manifest().id.clone();
    let root = env::var_os("RACKFORGE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/kalex/rackforge"));
    let mut selected = BTreeMap::<String, (Version, PluginPackage)>::new();

    let mut candidates = Vec::new();
    if let Ok(entries) = fs::read_dir(root.join("plugins")) {
        candidates.extend(entries.flatten().map(|entry| entry.path()));
    }
    if let Ok(plugin_entries) = fs::read_dir(root.join("plugin-store/packages")) {
        for plugin_entry in plugin_entries.flatten() {
            if let Ok(version_entries) = fs::read_dir(plugin_entry.path()) {
                candidates.extend(version_entries.flatten().map(|entry| entry.path()));
            }
        }
    }

    for candidate in candidates {
        if !candidate.join("rackforge-plugin.toml").is_file() {
            continue;
        }
        let package = match PluginPackage::open(&candidate) {
            Ok(package) => package,
            Err(error) => {
                eprintln!(
                    "PLUGIN_PACKAGE_IGNORED path={} error={error:#}",
                    candidate.display()
                );
                continue;
            }
        };
        if package.manifest().id == primary_id {
            continue;
        }
        let version = match Version::parse(&package.manifest().version) {
            Ok(version) => version,
            Err(error) => {
                eprintln!(
                    "PLUGIN_PACKAGE_IGNORED path={} error=invalid-version:{error}",
                    candidate.display()
                );
                continue;
            }
        };
        let replace = selected
            .get(&package.manifest().id)
            .is_none_or(|(current, _)| version > *current);
        if replace {
            selected.insert(package.manifest().id.clone(), (version, package));
        }
    }

    let mut packages = Vec::with_capacity(selected.len() + 1);
    packages.push(primary);
    packages.extend(selected.into_values().map(|(_, package)| package));
    Ok(packages)
}

fn plugin_instance_id(plugin_id: &str, primary: bool) -> Result<InstanceId> {
    if primary {
        return InstanceId::new(DEFAULT_LIVE_INSTANCE_ID)
            .map_err(|message| anyhow::anyhow!(message));
    }
    InstanceId::new(format!("play.{plugin_id}")).map_err(|message| anyhow::anyhow!(message))
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
    let (
        persisted_mode,
        persisted_active_instance_id,
        persisted_sound_id,
        persisted_master_level,
        persisted_master_pan,
        persisted_live,
    ) = match checkpoint.as_ref() {
        Some(store) => match (
            store.active_mode(&session_id),
            store.active_instance_id(&session_id),
            store.selected_sound(&session_id, DEFAULT_LIVE_INSTANCE_ID),
            store.master_level(&session_id),
            store.master_pan(&session_id),
            store.live_state(&session_id),
        ) {
            (
                Ok(mode),
                Ok(active_instance),
                Ok(sound_id),
                Ok(master_level),
                Ok(master_pan),
                Ok(live),
            ) => (
                mode,
                active_instance,
                sound_id,
                master_level,
                master_pan,
                live,
            ),
            (mode, active_instance, sound_id, master_level, master_pan, live) => {
                if let Err(error) = mode {
                    eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                }
                if let Err(error) = sound_id {
                    eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                }
                if let Err(error) = active_instance {
                    eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                }
                if let Err(error) = master_level {
                    eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                }
                if let Err(error) = master_pan {
                    eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                }
                if let Err(error) = live {
                    eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                }
                (None, None, None, None, None, None)
            }
        },
        None => (None, None, None, None, None, None),
    };
    let packages = discover_plugin_packages(&config.package)?;
    let primary_id = packages
        .first()
        .context("no primary plugin package was configured")?
        .manifest()
        .id
        .clone();
    let mut plugins = BTreeMap::<String, &'static LoadedPlugin>::new();
    for package in packages {
        let is_primary = package.manifest().id == primary_id;
        if package.manifest().kind != PluginKind::Instrument {
            eprintln!(
                "PLUGIN_PACKAGE_IGNORED id={} reason=kind:{:?}",
                package.manifest().id,
                package.manifest().kind
            );
            continue;
        }
        let resources = if is_primary {
            &config.resources
        } else {
            &BTreeMap::new()
        };
        let binary = is_primary.then_some(config.binary.as_deref()).flatten();
        // Native libraries remain loaded for the process lifetime. RackForge never
        // unloads a plugin while an audio instance may still reference its ABI.
        let loaded = match unsafe {
            LoadedPlugin::load(&package, binary, resources, config.data_root.as_deref())
        } {
            Ok(plugin) => Box::leak(Box::new(plugin)),
            Err(error) if !is_primary => {
                eprintln!(
                    "PLUGIN_RUNTIME_IGNORED id={} error={error:#}",
                    package.manifest().id
                );
                continue;
            }
            Err(error) => return Err(error),
        };
        println!(
            "LIVE_PLUGIN_READY id={} parameters={} presets={}",
            loaded.descriptor().id,
            loaded.parameters().parameters.len(),
            loaded.presets().presets.len()
        );
        plugins.insert(loaded.manifest().id.clone(), loaded);
    }
    let primary_plugin = *plugins
        .get(&primary_id)
        .context("primary plugin failed to load")?;

    let mut standalone_voices = Vec::with_capacity(plugins.len());
    let mut session_instances = Vec::with_capacity(plugins.len());
    let mut primary_preset_id = None;
    let mut primary_preset_name = None;
    for (plugin_id, plugin) in &plugins {
        let is_primary = plugin_id == &primary_id;
        let instance_id = plugin_instance_id(plugin_id, is_primary)?;
        let mut instance = plugin.create_instance()?;
        let presets = instance.preset_catalog()?;
        let secondary_persisted = (!is_primary)
            .then(|| {
                checkpoint
                    .as_ref()
                    .and_then(|store| store.selected_sound(&session_id, instance_id.as_str()).ok())
                    .flatten()
            })
            .flatten();
        let requested = if is_primary {
            persisted_sound_id.as_deref().or(config.preset.as_deref())
        } else {
            secondary_persisted.as_deref()
        };
        let selected = requested
            .and_then(|id| {
                presets
                    .presets
                    .iter()
                    .chain(plugin.presets().presets.iter())
                    .find(|preset| preset.id == id)
            })
            .or_else(|| presets.presets.first())
            .or_else(|| plugin.presets().presets.first());
        if let Some(preset) = selected {
            instance.load_preset(&preset.id)?;
            println!(
                "LIVE_PRESET_READY plugin={} id={} name={:?}",
                plugin_id, preset.id, preset.name
            );
            if is_primary {
                primary_preset_id = Some(preset.id.clone());
                primary_preset_name = Some(preset.name.clone());
            }
        }
        instance.activate(
            f64::from(output_rate),
            period_frames as u32,
            0,
            channels as u32,
        )?;
        session_instances.push(PluginInstanceState {
            instance_id: instance_id.clone(),
            plugin_id: plugin.manifest().id.clone(),
            plugin_name: plugin.manifest().name.clone(),
            ui_layouts: plugin.manifest().ui_layouts.clone(),
            config_available: plugin.manifest().config_mode,
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
                    editable: preset.editable,
                })
                .collect(),
            selected_sound_id: selected.map(|preset| preset.id.clone()),
        });
        standalone_voices.push(StandaloneVoice {
            instance_id,
            plugin,
            instance,
        });
    }
    let primary_preset_id =
        primary_preset_id.context("primary LIVE plugin exposes no program for the initial Rack")?;
    let primary_preset_name = primary_preset_name
        .context("primary LIVE plugin exposes no named program for the initial Rack")?;
    let primary_voice = standalone_voices
        .iter_mut()
        .find(|voice| voice.plugin.manifest().id == primary_id)
        .context("primary plugin instance is unavailable")?;
    let mut state_store = PluginStateStore::new(config.data_root.as_deref())?;
    let bootstrap_state = state_store.put(
        &primary_plugin.manifest().id,
        &primary_plugin.manifest().version,
        primary_plugin.manifest().state_version,
        Some(primary_preset_id.clone()),
        &primary_voice
            .instance
            .save_state()
            .context("capturing initial plugin state")?,
    )?;
    let mut performance_repository = PerformanceRepository::load_or_bootstrap(
        config.data_root.as_deref(),
        PerformanceBootstrap {
            plugin_id: primary_plugin.manifest().id.clone(),
            state: bootstrap_state,
            name: primary_preset_name,
        },
    )?;
    let migrated = performance_repository.migrate_legacy_plugin_states(
        &primary_plugin.manifest().id,
        |program_id| {
            let mut migration_instance = primary_plugin.create_instance()?;
            migration_instance.load_preset(program_id)?;
            let bytes = migration_instance.save_state()?;
            state_store.put(
                &primary_plugin.manifest().id,
                &primary_plugin.manifest().version,
                primary_plugin.manifest().state_version,
                Some(program_id.to_owned()),
                &bytes,
            )
        },
    )?;
    if migrated > 0 {
        println!("PERFORMANCE_PLUGIN_STATES_MIGRATED count={migrated}");
    }
    let performance_library = performance_repository.library().clone();
    let live_state = match persisted_live {
        Some(live) if live.validate(&performance_library).is_ok() => live,
        Some(_) => {
            eprintln!("SESSION_CHECKPOINT_LIVE_IGNORED reason=library-mismatch");
            performance_repository.initial_live_state()
        }
        None => performance_repository.initial_live_state(),
    };
    let mut initial_rack_specs = Vec::new();
    if let Some(rack) = live_state
        .active
        .as_ref()
        .and_then(|location| performance_repository.library().resolve(location).ok())
    {
        for slot in rack
            .slots
            .iter()
            .filter(|slot| slot.enabled)
            .take(control::MAX_ACTIVE_RACK_SLOTS)
        {
            let state = if let Some(reference) = &slot.state {
                RackSlotStateLoad::Opaque(state_store.read(reference)?)
            } else if let Some(program_id) = &slot.legacy_program_id {
                RackSlotStateLoad::LegacyPreset(program_id.clone())
            } else {
                RackSlotStateLoad::Default
            };
            initial_rack_specs.push(RackSlotRuntimeSpec {
                slot_id: slot.id.as_str().to_owned(),
                plugin_id: slot.plugin_id.clone(),
                state,
                midi_input_channel: slot.midi_input_channel,
                midi_note_low: slot.midi_note_low,
                midi_note_high: slot.midi_note_high,
                midi_transpose: slot.midi_transpose,
                keyboard_parts: rack.keyboard_parts,
                level_per_mille: slot.level_per_mille,
                pan_per_mille: slot.pan_per_mille,
            });
        }
    }
    let rack_voices = create_rack_voices(
        &plugins,
        &initial_rack_specs,
        output_rate,
        period_frames as u32,
        channels as u32,
    )?;

    let (sender, receiver) = mpsc::sync_channel(MIDI_QUEUE_CAPACITY);
    let (midi_port_names, midi_sources) = connect_midi_sources(sender)?;
    println!("MIDI_READY ports={midi_port_names:?}");
    let play_route = compile_default_play_route(
        &midi_sources,
        primary_plugin
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
    let primary_instance_id =
        InstanceId::new(DEFAULT_LIVE_INSTANCE_ID).map_err(|message| anyhow::anyhow!(message))?;
    let active_instance_id = persisted_active_instance_id
        .as_deref()
        .and_then(|id| InstanceId::new(id).ok())
        .filter(|id| {
            session_instances
                .iter()
                .any(|instance| instance.instance_id == *id)
        })
        .unwrap_or_else(|| primary_instance_id.clone());
    let initial_surface_mode = persisted_mode.unwrap_or(SurfaceMode::Live);
    let session = SessionState {
        schema_version: SESSION_SCHEMA_VERSION,
        session_id,
        revision: Revision::ZERO,
        active_mode: initial_surface_mode,
        master_level: persisted_master_level.unwrap_or(MasterLevel::UNITY),
        master_pan: persisted_master_pan.unwrap_or(MasterPan::CENTER),
        live: live_state,
        active_instance_id: Some(active_instance_id.clone()),
        instances: session_instances,
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
    let state_store = Arc::new(Mutex::new(state_store));
    let _control_server = control::start(
        &control_path,
        session_store,
        control_sender,
        Arc::clone(&audio_state),
        config.audio_state_path,
        Arc::new(Mutex::new(performance_repository)),
        state_store,
        plugins
            .values()
            .map(|plugin| (plugin.manifest().id.clone(), plugin.manifest().clone()))
            .collect(),
        control_storage,
        checkpoint,
    )?;
    println!("CONTROL_READY socket={}", control_path.display());
    println!("READY_TO_PLAY");
    audio_loop(
        output,
        &receiver,
        &control_receiver,
        &plugins,
        &mut standalone_voices,
        active_instance_id,
        rack_voices,
        &play_route,
        midi_port_names.len(),
        initial_master_level,
        initial_master_pan,
        initial_surface_mode.into(),
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

/// Enumerates the performance keyboards and hands them to a supervisor.
///
/// Discovery stays here, on the startup path, because the compiled routes need
/// the registry before the audio loop exists — and because failing when no
/// keyboard is present is deliberate: systemd restarts the unit, which is how a
/// board that boots before its USB devices enumerate recovers on its own.
///
/// Connections themselves move to [`midi_hotplug`], which keeps them alive
/// across replugging for the rest of the session.
fn connect_midi_sources(
    sender: SyncSender<IngressMidiEvent>,
) -> Result<(Vec<String>, MidiSourceRegistry)> {
    let discovery = MidiInput::new("rackforge-core-discovery")?;
    let names = performance_midi_names(&discovery)?;
    let mut registry = MidiSourceRegistry::default();
    let mut supervised = Vec::with_capacity(names.len());
    for (index, name) in names.iter().enumerate() {
        let key = MidiSourceKey::new(index as u32);
        let id = stable_alsa_source_id(name)?;
        registry.register(
            key,
            MidiSourceDescriptor {
                id: id.clone(),
                name: name.clone(),
                primary: index == 0,
            },
        )?;
        supervised.push(SupervisedSource {
            key,
            id,
            // Every source starts disconnected so the supervisor's first pass
            // performs the initial connection through the same path it will
            // use for every later reconnection. One code path, exercised from
            // the first second rather than only after a failure.
            connected: false,
        });
    }
    midi_hotplug::spawn(sender, supervised, midi_hotplug::DEFAULT_POLL_INTERVAL)?;
    Ok((names, registry))
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

fn standalone_voice_mut<'voices, 'plugin>(
    voices: &'voices mut [StandaloneVoice<'plugin>],
    instance_id: &InstanceId,
) -> Result<&'voices mut StandaloneVoice<'plugin>, String> {
    voices
        .iter_mut()
        .find(|voice| &voice.instance_id == instance_id)
        .ok_or_else(|| format!("unknown plugin instance {instance_id}"))
}

fn audio_loop<'plugin>(
    initial_output: OpenedAudioOutput,
    receiver: &Receiver<IngressMidiEvent>,
    control_receiver: &Receiver<AudioControlCommand>,
    plugins: &BTreeMap<String, &'plugin LoadedPlugin>,
    standalone_voices: &mut [StandaloneVoice<'plugin>],
    mut active_instance_id: InstanceId,
    mut rack_voices: Vec<RackSlotVoice<'plugin>>,
    play_route: &CompiledMidiRoute,
    midi_source_count: usize,
    initial_master_level: MasterLevel,
    initial_master_pan: MasterPan,
    mut render_mode: AudioRenderMode,
    audio_state: Arc<Mutex<AudioOutputState>>,
) -> Result<()> {
    let mut output = Some(initial_output);
    let mut period_frames = output.as_ref().unwrap().profile.period_frames as usize;
    let mut channels = output.as_ref().unwrap().profile.channels as usize;
    let mut output_rate = output.as_ref().unwrap().profile.sample_rate_hz as usize;
    let input = Vec::new();
    let mut plugin_output = vec![0.0_f32; period_frames * channels];
    let mut mix_output = vec![0.0_f32; period_frames * channels];
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
    let mut pending_emergency_stop = false;
    let mut master_gain = MasterGain::new(initial_master_level);
    let mut master_balance = MasterBalance::new(initial_master_pan);
    let mut reserved_midi_controls = ReservedMidiControls::with_sources(midi_source_count);

    // Engaged here, not during setup: `SCHED_FIFO` is a per-thread property and
    // this is the thread that runs the audio loop. Setup work stays on the
    // ordinary scheduler, where blocking on the filesystem is harmless.
    let realtime_status = realtime::engage(realtime::DEFAULT_AUDIO_PRIORITY);
    println!("{realtime_status}");
    if let Some(remedy) = realtime_status.remedy() {
        eprintln!("REALTIME_REMEDY {remedy}");
    }
    let mut xruns = XrunMonitor::new(output_rate as u32, period_frames);

    loop {
        while let Ok(command) = control_receiver.try_recv() {
            match command {
                AudioControlCommand::ApplyAudioOutput { profile, reply } => {
                    let result = reconfigure_audio_output(
                        &mut output,
                        standalone_voices,
                        &mut rack_voices,
                        profile,
                        &audio_state,
                    );
                    if let Ok(snapshot) = &result {
                        period_frames = snapshot.active_profile.period_frames as usize;
                        channels = snapshot.active_profile.channels as usize;
                        output_rate = snapshot.active_profile.sample_rate_hz as usize;
                        xruns.reconfigure(output_rate as u32, period_frames);
                        plugin_output.resize(period_frames * channels, 0.0);
                        mix_output.resize(period_frames * channels, 0.0);
                        for voice in &mut rack_voices {
                            voice.output.resize(period_frames * channels, 0.0);
                        }
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
                    reserved_midi_controls.replace(&bindings, &[]);
                    println!(
                        "HOST_CONTROLS_REGISTERED controller={controller_id} count={}",
                        bindings.len()
                    );
                    let _ = reply.send(Ok(()));
                }
                AudioControlCommand::RegisterHostBindings {
                    controller_id,
                    controls,
                    actions,
                    reply,
                } => {
                    reserved_midi_controls.replace(&controls, &actions);
                    println!(
                        "HOST_BINDINGS_REGISTERED controller={controller_id} controls={} actions={}",
                        controls.len(),
                        actions.len()
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
                AudioControlCommand::SetRenderMode { mode, reply } => {
                    let requested_mode = mode.into();
                    if requested_mode == AudioRenderMode::Silent {
                        render_mode = AudioRenderMode::Silent;
                        events.clear();
                        for voice in &mut rack_voices {
                            voice.events.clear();
                        }
                        replay_controller_state = false;
                        println!("AUDIO_RENDER_MODE mode={render_mode:?}");
                        let _ = reply.send(Ok(()));
                    } else {
                        let result = if render_mode == AudioRenderMode::Silent {
                            if pending_emergency_stop {
                                pending_emergency_stop = false;
                                if let Err(error) =
                                    stop_all_plugin_runtimes(standalone_voices, &mut rack_voices)
                                {
                                    eprintln!("EMERGENCY_STOP_RUNTIME_FAILED error={error}");
                                }
                            }
                            restart_render_target(
                                requested_mode,
                                standalone_voices,
                                &active_instance_id,
                                &mut rack_voices,
                                output_rate as f64,
                                period_frames as u32,
                                channels as u32,
                            )
                        } else {
                            Ok(())
                        };
                        if result.is_ok() {
                            render_mode = requested_mode;
                            replay_controller_state = true;
                            println!("AUDIO_RENDER_MODE mode={render_mode:?}");
                        }
                        let _ = reply.send(result);
                    }
                }
                AudioControlCommand::EmergencyStop { reply } => {
                    render_mode = AudioRenderMode::Silent;
                    events.clear();
                    for voice in &mut rack_voices {
                        voice.events.clear();
                    }
                    audition = None;
                    replay_controller_state = false;
                    pending_emergency_stop = true;
                    println!("AUDIO_EMERGENCY_STOP output=silent runtime_stop=pending");
                    println!("AUDIO_RENDER_MODE mode={render_mode:?}");
                    let _ = reply.send(Ok(()));
                }
                AudioControlCommand::SelectPlugin { instance_id, reply } => {
                    let result = (|| -> Result<(), String> {
                        if active_instance_id == instance_id {
                            return Ok(());
                        }
                        standalone_voice_mut(standalone_voices, &instance_id)?;
                        standalone_voice_mut(standalone_voices, &active_instance_id)?
                            .instance
                            .reset()
                            .map_err(|error| error.to_string())?;
                        active_instance_id = instance_id.clone();
                        replay_controller_state |= render_mode == AudioRenderMode::Plugin;
                        println!("LIVE_PLUGIN_SELECTED instance={instance_id}");
                        Ok(())
                    })();
                    let _ = reply.send(result);
                }
                AudioControlCommand::SelectSound {
                    instance_id,
                    sound_id,
                    reply,
                } => {
                    let result = standalone_voice_mut(standalone_voices, &instance_id)
                        .and_then(|voice| {
                            voice
                                .instance
                                .load_preset(&sound_id)
                                .map_err(|error| error.to_string())
                        })
                        .map(|()| {
                            println!("LIVE_SOUND_SELECTED instance={instance_id} id={sound_id}");
                        });
                    if result.is_ok() {
                        active_instance_id = instance_id;
                        render_mode = AudioRenderMode::Plugin;
                    }
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result.map_err(|error| error.to_string()));
                }
                AudioControlCommand::CaptureState { instance_id, reply } => {
                    let result =
                        standalone_voice_mut(standalone_voices, &instance_id).and_then(|voice| {
                            voice
                                .instance
                                .save_state()
                                .map_err(|error| error.to_string())
                        });
                    let _ = reply.send(result);
                }
                AudioControlCommand::RestoreState {
                    instance_id,
                    bytes,
                    reply,
                } => {
                    let result =
                        standalone_voice_mut(standalone_voices, &instance_id).and_then(|voice| {
                            voice
                                .instance
                                .load_state(&bytes)
                                .map_err(|error| error.to_string())
                        });
                    if result.is_ok() {
                        active_instance_id = instance_id;
                        render_mode = AudioRenderMode::Plugin;
                        replay_controller_state = true;
                    }
                    let _ = reply.send(result);
                }
                AudioControlCommand::MaterializeState {
                    instance_id,
                    program_id,
                    reply,
                } => {
                    let result = (|| -> Result<Vec<u8>, String> {
                        let plugin = standalone_voice_mut(standalone_voices, &instance_id)?.plugin;
                        let mut snapshot = plugin
                            .create_instance()
                            .map_err(|error| error.to_string())?;
                        if let Some(program_id) = program_id {
                            snapshot
                                .load_preset(&program_id)
                                .map_err(|error| error.to_string())?;
                        }
                        snapshot.save_state().map_err(|error| error.to_string())
                    })();
                    let _ = reply.send(result);
                }
                AudioControlCommand::PluginParameters { instance_id, reply } => {
                    let result =
                        standalone_voice_mut(standalone_voices, &instance_id).and_then(|voice| {
                            let schema = voice.plugin.parameters().clone();
                            let values = schema
                                .parameters
                                .iter()
                                .map(|parameter| {
                                    voice
                                        .instance
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
                    let _ = reply.send(result);
                }
                AudioControlCommand::SetPluginParameter {
                    instance_id,
                    parameter_index,
                    value,
                    reply,
                } => {
                    let result =
                        standalone_voice_mut(standalone_voices, &instance_id).and_then(|voice| {
                            let parameter = voice
                                .plugin
                                .parameters()
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
                                .instance
                                .set_parameter(parameter_index, value)
                                .map_err(|error| error.to_string())?;
                            voice
                                .instance
                                .get_parameter(parameter_index)
                                .map_err(|error| error.to_string())
                        });
                    let _ = reply.send(result);
                }
                AudioControlCommand::ActivateRack {
                    rack_id,
                    instance_id,
                    slots,
                    reply,
                } => {
                    let result = create_rack_voices(
                        plugins,
                        &slots,
                        output_rate as u32,
                        period_frames as u32,
                        channels as u32,
                    )
                    .map(|voices| {
                        rack_voices = voices;
                        render_mode = AudioRenderMode::Rack;
                        println!(
                            "LIVE_RACK_ACTIVATED rack={rack_id} instance={instance_id} slots={}",
                            rack_voices.len()
                        );
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
                        standalone_voice_mut(standalone_voices, &instance_id)?
                            .instance
                            .reset()
                            .map_err(|error| error.to_string())?;
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
                    if result.is_ok() {
                        active_instance_id = instance_id;
                        render_mode = AudioRenderMode::Plugin;
                    }
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
                            standalone_voice_mut(standalone_voices, &lease.instance_id)
                                .and_then(|voice| {
                                    restore_after_audition(&mut voice.instance, &lease)
                                        .map_err(|error| error.to_string())
                                })
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
                    if result.is_ok() {
                        render_mode = AudioRenderMode::Plugin;
                    }
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
                        let instance =
                            &mut standalone_voice_mut(standalone_voices, &instance_id)?.instance;
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
                    if result.is_ok() {
                        active_instance_id = instance_id;
                        render_mode = AudioRenderMode::Plugin;
                    }
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result);
                }
                AudioControlCommand::ReplaceProgramDraft {
                    instance_id,
                    document,
                    reply,
                } => {
                    let result = (|| -> Result<_, String> {
                        let instance =
                            &mut standalone_voice_mut(standalone_voices, &instance_id)?.instance;
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
                    if result.is_ok() {
                        render_mode = AudioRenderMode::Plugin;
                    }
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result);
                }
                AudioControlCommand::EditProgramDraftField {
                    instance_id,
                    request,
                    reply,
                } => {
                    let result = (|| -> Result<_, String> {
                        let instance =
                            &mut standalone_voice_mut(standalone_voices, &instance_id)?.instance;
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
                    if result.is_ok() {
                        render_mode = AudioRenderMode::Plugin;
                    }
                    replay_controller_state |= result.is_ok();
                    let _ = reply.send(result);
                }
                AudioControlCommand::InstallProgram {
                    instance_id,
                    prepared,
                    reply,
                } => {
                    let result = standalone_voice_mut(standalone_voices, &instance_id)
                        .and_then(|voice| {
                            voice
                                .instance
                                .install_program(&prepared)
                                .and_then(|()| voice.instance.preset_catalog())
                                .map_err(|error| error.to_string())
                        })
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
                    let result = standalone_voice_mut(standalone_voices, &instance_id)
                        .and_then(|voice| {
                            voice
                                .instance
                                .activate_surface(&request)
                                .map_err(|error| error.to_string())
                        })
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
        for voice in &mut rack_voices {
            voice.events.clear();
        }
        if replay_controller_state {
            let omitted = match render_mode {
                AudioRenderMode::Silent => 0,
                AudioRenderMode::Plugin => controller_states.replay_routed_into(
                    play_route,
                    None,
                    &mut events,
                    MAX_EVENTS_PER_BLOCK,
                ),
                AudioRenderMode::Rack => rack_voices
                    .iter_mut()
                    .map(|voice| {
                        controller_states.replay_routed_into(
                            play_route,
                            voice.midi_input_channel,
                            &mut voice.events,
                            MAX_EVENTS_PER_BLOCK,
                        )
                    })
                    .sum(),
            };
            dropped_events += omitted;
            if omitted > 0 {
                eprintln!("MIDI_CONTROLLER_REPLAY_TRUNCATED omitted={omitted}");
            }
            replay_controller_state = false;
        }
        while let Ok(event) = receiver.try_recv() {
            let plugin_event = plugin_midi_event(event.packet);
            if reserved_midi_controls.consume(event.source, plugin_event) {
                continue;
            }
            controller_states.observe(event.source, plugin_event);
            match render_mode {
                AudioRenderMode::Silent => {}
                AudioRenderMode::Plugin => {
                    if let Some(routed) = play_route.route(event) {
                        if events.len() < MAX_EVENTS_PER_BLOCK {
                            events.push(plugin_midi_event(routed.packet));
                        } else {
                            dropped_events += 1;
                        }
                    }
                }
                AudioRenderMode::Rack => {
                    for voice in &mut rack_voices {
                        if let Some(routed) = route_rack_event(
                            event,
                            voice.midi_input_channel,
                            voice.midi_note_low,
                            voice.midi_note_high,
                            voice.midi_transpose,
                            voice.keyboard_parts,
                            play_route,
                        ) {
                            if voice.events.len() < MAX_EVENTS_PER_BLOCK {
                                voice.events.push(routed);
                            } else {
                                dropped_events += 1;
                            }
                        }
                    }
                }
            }
        }

        mix_output.fill(0.0);
        match render_mode {
            AudioRenderMode::Silent => {}
            AudioRenderMode::Plugin => {
                plugin_output.fill(0.0);
                let process_result = standalone_voice_mut(standalone_voices, &active_instance_id)
                    .map_err(anyhow::Error::msg)?
                    .instance
                    .process_interleaved(
                        &input,
                        &mut plugin_output,
                        period_frames as u32,
                        0,
                        channels as u32,
                        &events,
                        &[],
                    );
                if quarantine_failed_process(
                    process_result,
                    &mut plugin_output,
                    &format!("standalone:{active_instance_id}"),
                ) {
                    render_mode = AudioRenderMode::Silent;
                } else {
                    mix_output.copy_from_slice(&plugin_output);
                }
            }
            AudioRenderMode::Rack => {
                for voice in &mut rack_voices {
                    voice.output.fill(0.0);
                    if voice.process_faulted {
                        continue;
                    }
                    let process_result = voice.instance.process_interleaved(
                        &input,
                        &mut voice.output,
                        period_frames as u32,
                        0,
                        channels as u32,
                        &voice.events,
                        &[],
                    );
                    if quarantine_failed_process(
                        process_result,
                        &mut voice.output,
                        &format!("rack-slot:{}", voice.slot_id),
                    ) {
                        voice.process_faulted = true;
                        continue;
                    }
                    mix_rack_slot(
                        &mut mix_output,
                        &voice.output,
                        channels,
                        voice.level,
                        voice.pan,
                    );
                }
            }
        }
        for (source_frame, target_frame) in mix_output
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
            &mut xruns,
        )?;
        if let Some(report) = xruns.tick() {
            eprintln!("{report}");
        }

        if pending_emergency_stop {
            pending_emergency_stop = false;
            if let Err(error) = stop_all_plugin_runtimes(standalone_voices, &mut rack_voices) {
                eprintln!("EMERGENCY_STOP_RUNTIME_FAILED error={error}");
            }
            println!("AUDIO_EMERGENCY_STOP runtime_stop=complete");
        }

        if meter_frames >= output_rate {
            let midi_events = match render_mode {
                AudioRenderMode::Silent => 0,
                AudioRenderMode::Plugin => events.len(),
                AudioRenderMode::Rack => rack_voices.iter().map(|voice| voice.events.len()).sum(),
            };
            println!(
                "AUDIO_METER peak={meter_peak:.3} clipped={meter_clipped} \
                 midi_events={} dropped_events={dropped_events}",
                midi_events
            );
            meter_frames = 0;
            meter_peak = 0.0;
            meter_clipped = 0;
            dropped_events = 0;
        }
    }
}

fn quarantine_failed_process<E: std::fmt::Display>(
    result: std::result::Result<(), E>,
    output: &mut [f32],
    context: &str,
) -> bool {
    let Err(error) = result else {
        return false;
    };
    output.fill(0.0);
    eprintln!("PLUGIN_PROCESS_QUARANTINED context={context} action=silence error={error}");
    true
}

fn stop_all_plugin_runtimes(
    standalone_voices: &mut [StandaloneVoice<'_>],
    rack_voices: &mut [RackSlotVoice<'_>],
) -> Result<(), String> {
    let mut failures = Vec::new();
    for voice in standalone_voices {
        if let Err(error) = replace_with_stopped_runtime(voice.plugin, &mut voice.instance) {
            failures.push(format!("instance {}: {error}", voice.instance_id));
        }
    }
    for voice in rack_voices {
        if let Err(error) = replace_with_stopped_runtime(voice.plugin, &mut voice.instance) {
            failures.push(format!("slot {}: {error}", voice.slot_id));
        }
        voice.events.clear();
        voice.output.fill(0.0);
        voice.process_faulted = false;
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn replace_with_stopped_runtime<'plugin>(
    plugin: &'plugin LoadedPlugin,
    instance: &mut PluginInstance<'plugin>,
) -> Result<(), String> {
    let state = instance.save_state().map_err(|error| error.to_string())?;
    let mut replacement = plugin
        .create_instance()
        .map_err(|error| error.to_string())?;
    replacement
        .load_state(&state)
        .map_err(|error| error.to_string())?;
    instance.deactivate().map_err(|error| error.to_string())?;
    *instance = replacement;
    Ok(())
}

fn restart_render_target(
    mode: AudioRenderMode,
    standalone_voices: &mut [StandaloneVoice<'_>],
    active_instance_id: &InstanceId,
    rack_voices: &mut [RackSlotVoice<'_>],
    sample_rate: f64,
    maximum_frames: u32,
    output_channels: u32,
) -> Result<(), String> {
    match mode {
        AudioRenderMode::Silent => Ok(()),
        AudioRenderMode::Plugin => standalone_voice_mut(standalone_voices, active_instance_id)?
            .instance
            .activate(sample_rate, maximum_frames, 0, output_channels)
            .map_err(|error| error.to_string()),
        AudioRenderMode::Rack => {
            for index in 0..rack_voices.len() {
                let activation = {
                    let voice = &mut rack_voices[index];
                    voice
                        .instance
                        .activate(sample_rate, maximum_frames, 0, output_channels)
                        .map_err(|error| format!("reactivating slot {}: {error:#}", voice.slot_id))
                };
                if let Err(error) = activation {
                    for activated in &mut *rack_voices {
                        let _ = activated.instance.deactivate();
                    }
                    return Err(error);
                }
            }
            Ok(())
        }
    }
}

fn parameter_value_is_valid(kind: &ParameterKind, value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    match kind {
        ParameterKind::Float {
            minimum, maximum, ..
        }
        | ParameterKind::Meter {
            minimum, maximum, ..
        } => (*minimum..=*maximum).contains(&value),
        ParameterKind::Integer {
            minimum,
            maximum,
            step,
            ..
        } => {
            value.fract() == 0.0
                && value >= *minimum as f64
                && value <= *maximum as f64
                && ((value as i64 - *minimum) % *step == 0)
        }
        ParameterKind::Boolean { .. } => value == 0.0 || value == 1.0,
        ParameterKind::Enum { choices, .. } => {
            value.fract() == 0.0
                && choices
                    .iter()
                    .any(|choice| f64::from(choice.value) == value)
        }
        ParameterKind::Trigger => value == 0.0 || value == 1.0,
    }
}

fn mix_rack_slot(mix: &mut [f32], source: &[f32], channels: usize, level: f32, pan: f32) {
    let left = level * (1.0 - pan.max(0.0));
    let right = level * (1.0 + pan.min(0.0));
    for (source_frame, mix_frame) in source
        .chunks_exact(channels)
        .zip(mix.chunks_exact_mut(channels))
    {
        for (channel, (sample, target)) in source_frame.iter().zip(mix_frame).enumerate() {
            *target += sample * if channel == 0 { left } else { right };
        }
    }
}

fn reconfigure_audio_output(
    output: &mut Option<OpenedAudioOutput>,
    standalone_voices: &mut [StandaloneVoice<'_>],
    rack_voices: &mut [RackSlotVoice<'_>],
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

    for voice in standalone_voices.iter_mut() {
        voice
            .instance
            .deactivate()
            .with_context(|| format!("deactivating plugin instance {}", voice.instance_id))?;
    }
    for voice in rack_voices.iter_mut() {
        voice
            .instance
            .deactivate()
            .with_context(|| format!("deactivating Rack Slot {}", voice.slot_id))?;
    }
    drop(output.take());
    let apply = (|| -> Result<(OpenedAudioOutput, Vec<_>)> {
        let devices = discover_audio_devices().context("refreshing audio device inventory")?;
        let opened = open_audio_output_from_inventory(&requested, &devices)?;
        for voice in standalone_voices.iter_mut() {
            voice
                .instance
                .activate(
                    f64::from(requested.sample_rate_hz),
                    requested.period_frames,
                    0,
                    requested.channels,
                )
                .with_context(|| format!("activating plugin instance {}", voice.instance_id))?;
        }
        for voice in rack_voices.iter_mut() {
            voice
                .instance
                .activate(
                    f64::from(requested.sample_rate_hz),
                    requested.period_frames,
                    0,
                    requested.channels,
                )
                .with_context(|| format!("activating Rack Slot {}", voice.slot_id))?;
        }
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
                for voice in standalone_voices.iter_mut() {
                    voice.instance.activate(
                        f64::from(previous_profile.sample_rate_hz),
                        previous_profile.period_frames,
                        0,
                        previous_profile.channels,
                    )?;
                }
                for voice in rack_voices.iter_mut() {
                    voice.instance.activate(
                        f64::from(previous_profile.sample_rate_hz),
                        previous_profile.period_frames,
                        0,
                        previous_profile.channels,
                    )?;
                }
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

fn matches_midi_input_channel(packet: MidiPacket, channel: Option<u8>) -> bool {
    channel.is_none_or(|channel| packet.channel().user_number() == channel)
}

fn route_rack_event(
    event: IngressMidiEvent,
    midi_input_channel: Option<u8>,
    midi_note_low: u8,
    midi_note_high: u8,
    midi_transpose: i8,
    keyboard_parts: Option<RackKeyboardParts>,
    play_route: &CompiledMidiRoute,
) -> Option<MidiEventV1> {
    let status = event.packet.data[0] & 0xf0;
    let keyed_message = matches!(status, 0x80 | 0x90 | 0xa0) && event.packet.length >= 2;
    let part_transpose = if let Some(parts) = keyboard_parts {
        let part = if keyed_message {
            let note = event.packet.data[1];
            match parts.split_key {
                Some(split) if note >= split => parts.part_2,
                _ => parts.part_1,
            }
        } else if parts.split_key.is_some() && midi_input_channel == Some(parts.part_2.midi_channel)
        {
            parts.part_2
        } else {
            parts.part_1
        };
        if midi_input_channel.is_some_and(|channel| channel != part.midi_channel) {
            return None;
        }
        part.transpose
    } else {
        if !matches_midi_input_channel(event.packet, midi_input_channel) {
            return None;
        }
        0
    };
    if keyed_message {
        let note = event.packet.data[1];
        if !(midi_note_low..=midi_note_high).contains(&note) {
            return None;
        }
    }
    let mut packet = play_route.route(event)?.packet;
    if keyed_message {
        let transposed =
            i16::from(packet.data[1]) + i16::from(part_transpose) + i16::from(midi_transpose);
        if !(0..=127).contains(&transposed) {
            return None;
        }
        packet.data[1] = transposed as u8;
    }
    Some(plugin_midi_event(packet))
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
    xruns: &mut XrunMonitor,
) -> Result<()> {
    let mut frame_offset = 0;
    while frame_offset < period_frames {
        match io.writei(&output[frame_offset * channels..]) {
            Ok(0) => bail!("audio output accepted zero frames"),
            Ok(frames) => frame_offset += frames,
            Err(error) if error.errno() == libc::EPIPE => {
                // Counted rather than printed: a dropout storm that logs per
                // underrun blocks this thread on stderr and causes the next one.
                xruns.record();
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
    use rackforge_midi_api::MidiSourceId;
    use rackforge_performance_api::RackKeyboardPart;
    use rackforge_session_api::{
        HostActionBinding, HostActionTarget, HostControlTarget, MidiButtonBinding,
        MidiControlChangeBinding,
    };

    fn midi(length: u8, data: [u8; 3]) -> MidiEventV1 {
        MidiEventV1 {
            frame: 0,
            length,
            data,
        }
    }

    #[test]
    fn plugin_process_failure_is_silenced_and_quarantined() {
        let mut output = [0.25_f32, -0.5, 0.75, -1.0];
        assert!(quarantine_failed_process(
            Err("all fuel consumed"),
            &mut output,
            "test"
        ));
        assert_eq!(output, [0.0; 4]);

        let mut successful = [0.25_f32, -0.5];
        assert!(!quarantine_failed_process(
            Ok::<(), &str>(()),
            &mut successful,
            "test"
        ));
        assert_eq!(successful, [0.25, -0.5]);
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
    fn rack_slot_midi_channel_accepts_omni_or_only_the_selected_channel() {
        let channel_one = MidiPacket::new(0, &[0x90, 60, 100]).unwrap();
        let channel_two = MidiPacket::new(0, &[0x91, 60, 100]).unwrap();

        assert!(matches_midi_input_channel(channel_one, None));
        assert!(matches_midi_input_channel(channel_two, None));
        assert!(matches_midi_input_channel(channel_one, Some(1)));
        assert!(!matches_midi_input_channel(channel_two, Some(1)));
        assert!(matches_midi_input_channel(channel_two, Some(2)));
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
    fn rack_slot_route_applies_channel_zone_and_transposition_before_the_plugin() {
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
        let route = compile_default_play_route(&sources, PluginChannelModel::SinglePart).unwrap();
        let event = |message: &[u8]| IngressMidiEvent {
            source: MidiSourceKey::new(0),
            packet: MidiPacket::new(0, message).unwrap(),
        };

        let routed = route_rack_event(event(&[0x91, 48, 100]), Some(2), 36, 59, 12, None, &route)
            .expect("note inside the Part should be routed");
        assert_eq!(routed.data, [0x90, 60, 100]);

        assert!(
            route_rack_event(event(&[0x91, 60, 100]), Some(2), 36, 59, 12, None, &route).is_none()
        );
        assert!(
            route_rack_event(event(&[0x90, 48, 100]), Some(2), 36, 59, 12, None, &route).is_none()
        );

        let modulation = route_rack_event(event(&[0xb1, 1, 64]), Some(2), 36, 59, 12, None, &route)
            .expect("non-note expression should follow the Part channel");
        assert_eq!(modulation.data, [0xb0, 1, 64]);
    }

    #[test]
    fn keyboard_parts_split_and_remap_before_slot_channel_filtering() {
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
        let route = compile_default_play_route(&sources, PluginChannelModel::SinglePart).unwrap();
        let event = |message: &[u8]| IngressMidiEvent {
            source: MidiSourceKey::new(0),
            packet: MidiPacket::new(0, message).unwrap(),
        };
        let parts = RackKeyboardParts {
            split_key: Some(60),
            part_1: RackKeyboardPart {
                midi_channel: 1,
                transpose: 0,
            },
            part_2: RackKeyboardPart {
                midi_channel: 2,
                transpose: 12,
            },
        };

        assert!(
            route_rack_event(
                event(&[0x95, 48, 100]),
                Some(1),
                0,
                127,
                0,
                Some(parts),
                &route
            )
            .is_some()
        );
        assert!(
            route_rack_event(
                event(&[0x95, 48, 100]),
                Some(2),
                0,
                127,
                0,
                Some(parts),
                &route
            )
            .is_none()
        );
        assert!(
            route_rack_event(
                event(&[0x95, 64, 100]),
                Some(1),
                0,
                127,
                0,
                Some(parts),
                &route
            )
            .is_none()
        );
        let piano = route_rack_event(
            event(&[0x95, 64, 100]),
            Some(2),
            0,
            127,
            0,
            Some(parts),
            &route,
        )
        .expect("right Part should reach the CH2 Slot");
        assert_eq!(piano.data, [0x90, 76, 100]);

        let no_split = RackKeyboardParts {
            split_key: None,
            ..parts
        };
        assert!(
            route_rack_event(
                event(&[0x95, 64, 100]),
                Some(2),
                0,
                127,
                0,
                Some(no_split),
                &route,
            )
            .is_none()
        );
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
        reserved.replace(
            &[HostControlBinding {
                target: HostControlTarget::MasterLevel,
                midi_cc: MidiControlChangeBinding {
                    channel: 0,
                    controller: 82,
                },
            }],
            &[],
        );

        let source = MidiSourceKey::new(0);
        assert!(reserved.consume(source, midi(3, [0xb0, 82, 64])));
        assert!(!reserved.consume(source, midi(3, [0xb0, 83, 64])));
        assert!(!reserved.consume(source, midi(3, [0xb1, 82, 64])));
        assert!(!reserved.consume(source, midi(3, [0x90, 82, 64])));

        reserved.replace(&[], &[]);
        assert!(!reserved.consume(source, midi(3, [0xb0, 82, 64])));
    }

    #[test]
    fn reserved_host_action_never_reaches_plugin_midi() {
        let mut reserved = ReservedMidiControls::default();
        reserved.replace(
            &[],
            &[HostActionBinding {
                target: HostActionTarget::KeyboardParts,
                midi_cc: MidiButtonBinding {
                    channel: 0,
                    controller: 119,
                    press_value: 127,
                    release_value: 0,
                },
            }],
        );

        let source = MidiSourceKey::new(0);
        assert!(reserved.consume(source, midi(3, [0xb0, 119, 127])));
        assert!(reserved.consume(source, midi(3, [0x90, 60, 100])));
        assert!(reserved.consume(source, midi(3, [0x80, 60, 0])));
        assert!(reserved.consume(source, midi(3, [0xb0, 119, 0])));
        assert!(!reserved.consume(source, midi(3, [0x90, 61, 100])));
        assert!(!reserved.consume(source, midi(3, [0xb0, 118, 127])));
        assert!(!reserved.consume(source, midi(3, [0xb1, 119, 127])));
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

    #[test]
    fn web_parameter_values_are_checked_against_the_public_schema() {
        let float = ParameterKind::Float {
            minimum: 0.0,
            maximum: 1.0,
            default: 0.5,
            step: 0.01,
            unit: None,
        };
        assert!(parameter_value_is_valid(&float, 0.625));
        assert!(!parameter_value_is_valid(&float, 1.1));
        assert!(!parameter_value_is_valid(&float, f64::NAN));

        let integer = ParameterKind::Integer {
            minimum: 0,
            maximum: 8,
            default: 0,
            step: 2,
            unit: None,
        };
        assert!(parameter_value_is_valid(&integer, 6.0));
        assert!(!parameter_value_is_valid(&integer, 5.0));
    }
}
