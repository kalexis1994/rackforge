use crate::audio::{
    OpenedAudioInput, OpenedAudioOutput, discover_audio_devices, open_audio_input_from_inventory,
    open_audio_output_from_inventory,
};
use crate::control::{
    self, AudioControlCommand, MAX_EVENTS_PER_BLOCK, RackMidiStageRuntimeSpec, RackSlotRuntimeSpec,
    RackSlotStateLoad,
};
use crate::isolated_state::parameter_value_is_valid;
use crate::live_midi_state::{MidiControllerStates, ReservedMidiControls, plugin_midi_event};
use crate::midi_hotplug::{
    self, SupervisedSource, is_performance_midi_input, stable_alsa_source_id,
};
use crate::parallel_render::{
    self, ParallelUnits, RenderPool, RenderTelemetry, ScheduledSlot, UnitJob,
    process_slots_sequential, spawn_telemetry_publisher,
};
use crate::performance::PerformanceRepository;
use crate::rack_graph::compile_instrument_definition;
use crate::realtime::{self, XrunMonitor};
use crate::session::SessionStore;
use crate::session_checkpoint::SessionCheckpointStore;
use crate::{
    CompiledParameterLink, LiveParameterStateStore, LiveParameterTarget, LiveParameterWriter,
    LiveParameterWriterHandle, LoadedPlugin, PluginInstance, PluginPackage, PluginStateStore,
};
use alsa::pcm::PCM;
use anyhow::{Context, Result, bail};
use midir::MidiInput;
use rackforge_audio_api::{
    AUDIO_OUTPUT_STATE_SCHEMA_VERSION, AudioInputProfile, AudioOutputProfile, AudioOutputState,
    AudioSampleFormat, OutputMeter,
};
use rackforge_control_api::{CONTROL_SOCKET_NAME, PluginParameterValue};
use rackforge_midi_api::{
    CompiledMidiRoute, DEFAULT_INPUT_BUS_ID, IngressMidiEvent, MIDI_ROUTING_SCHEMA_VERSION,
    MidiInputBusId, MidiPacket, MidiRoute, MidiRouteId, MidiRouteMatch, MidiRouteTarget,
    MidiRouteTransform,
    MidiSourceDescriptor, MidiSourceId, MidiSourceKey, MidiSourceRegistry, MidiSourceSelector,
    MidiTargetId, ParameterLink, ParameterLinkPassThrough, PluginChannelModel,
};
#[cfg(test)]
use rackforge_performance_api::RackKeyboardParts;
use rackforge_plugin_api::abi::{MidiEventV1, ParameterEventV1};
use rackforge_plugin_api::{ParameterKind, PluginKind};
use rackforge_session_api::{
    BankSummary, DEFAULT_LIVE_INSTANCE_ID, DEFAULT_LIVE_SESSION_ID, InstanceId, MasterLevel,
    MasterPan, PluginInstanceState, Revision, SESSION_SCHEMA_VERSION, SessionId, SessionState,
    SoundSummary, SurfaceMode,
};
use semver::Version;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

const MIDI_QUEUE_CAPACITY: usize = 2_048;
const AUDIO_CONTROL_QUEUE_CAPACITY: usize = 64;
const MASTER_LEVEL_SMOOTHING_FRAMES: u32 = 480;
pub(crate) const VIRTUAL_MIDI_SOURCE_ID: &str = "rackforge.virtual.touch";

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

fn resolve_render_mode(mode: SurfaceMode, rack_voice_count: usize) -> AudioRenderMode {
    match mode {
        SurfaceMode::Live if rack_voice_count == 0 => AudioRenderMode::Silent,
        _ => mode.into(),
    }
}

/// One audio source of a Slot, resolved to indices once per activation so
/// the per-block gather performs no string comparisons.
#[derive(Clone, Copy)]
enum ResolvedRackSource {
    /// The hardware capture staged for the current block.
    Capture,
    /// The finished output of an earlier Slot in the compiled order.
    Slot(usize),
}

struct RackSlotVoice<'plugin> {
    slot_id: String,
    plugin: &'plugin LoadedPlugin,
    instance: PluginInstance<'plugin>,
    /// Host-owned unit instances for `parallel_render_v1` plugins; `None`
    /// keeps the Slot on the classic indivisible render path.
    parallel: Option<ParallelUnits<'plugin>>,
    midi_stages: Vec<RackMidiStageRuntimeSpec>,
    audio_sources: Vec<crate::rack_graph::CompiledAudioSource>,
    /// `audio_sources` resolved against the compiled Slot order.
    resolved_sources: Vec<ResolvedRackSource>,
    /// Bitmask of the earlier Slots feeding this one; the scheduler holds
    /// this Slot until every one of them completed its block.
    deps_mask: u32,
    /// Hardware capture staged for the current block; rewritten by the
    /// audio loop before every Rack render.
    capture_ptr: *const f32,
    capture_len: usize,
    capture_channels: usize,
    sends_to_main: bool,
    input_channels: usize,
    level: f32,
    pan: f32,
    input: Vec<f32>,
    output: Vec<f32>,
    events: Vec<MidiEventV1>,
    parameter_events: Vec<ParameterEventV1>,
    process_faulted: bool,
}

/// Resolves every Slot's cable sources to indices and dependency masks.
/// Runs at activation, never per block. A source that does not name an
/// earlier Slot is dropped, exactly as the previous sequential graph walk
/// ignored it: the compiled order is topological, so a forward reference
/// would be a compiler bug rather than a playable graph.
fn resolve_rack_voice_graph(voices: &mut [RackSlotVoice<'_>]) {
    for index in 0..voices.len() {
        let (earlier, rest) = voices.split_at_mut(index);
        let voice = &mut rest[0];
        voice.resolved_sources.clear();
        voice.deps_mask = 0;
        for source in &voice.audio_sources {
            match source {
                crate::rack_graph::CompiledAudioSource::HardwareInput { .. } => {
                    voice.resolved_sources.push(ResolvedRackSource::Capture);
                }
                crate::rack_graph::CompiledAudioSource::Slot { runtime_slot_id } => {
                    if let Some(upstream) = earlier
                        .iter()
                        .position(|candidate| candidate.slot_id == *runtime_slot_id)
                    {
                        voice
                            .resolved_sources
                            .push(ResolvedRackSource::Slot(upstream));
                        voice.deps_mask |= 1 << upstream;
                    } else {
                        eprintln!(
                            "LIVE_RACK_SOURCE_IGNORED slot={} source={runtime_slot_id}                              reason=not-an-earlier-slot",
                            voice.slot_id
                        );
                    }
                }
            }
        }
    }
}

struct PreparedPortableRackVoices(Vec<RackSlotVoice<'static>>);

// SAFETY: this wrapper is created only after checking that every voice uses
// RackForge's portable wasm-v1 backend. Ownership then moves from the audio
// loop to the reclaimer thread solely for destruction.
unsafe impl Send for PreparedPortableRackVoices {}

enum RetiredAudioRuntime {
    Standalone(control::PreparedPluginInstance),
    PortableRack(PreparedPortableRackVoices),
}

fn retire_portable_rack(
    voices: Vec<RackSlotVoice<'static>>,
    deferred: &mut Vec<RetiredAudioRuntime>,
) -> Result<(), Vec<RackSlotVoice<'static>>> {
    if voices.is_empty() {
        return Ok(());
    }
    if voices
        .iter()
        .all(|voice| voice.plugin.manifest().portable_component().is_some())
    {
        deferred.push(RetiredAudioRuntime::PortableRack(
            PreparedPortableRackVoices(voices),
        ));
        Ok(())
    } else {
        Err(voices)
    }
}

/// One Slot as the global scheduler sees it: classic plugins contribute a
/// single indivisible job, `parallel_render_v1` plugins contribute their
/// begin → units → end family.
//
// SAFETY: rack voices reach worker threads only through the pool's epoch
// protocol; unit jobs point at per-unit boxed cells inside `ParallelUnits`,
// which hold isolated portable instances. Classic processing has always run
// on pool workers, which the plugin ABI already requires plugins to accept.
unsafe impl<'plugin> ScheduledSlot for RackSlotVoice<'plugin> {
    fn max_units(&self) -> u32 {
        self.parallel.as_ref().map_or(0, ParallelUnits::max_units)
    }

    fn dependency_mask(&self) -> u32 {
        self.deps_mask
    }

    unsafe fn gather_input(
        slot_index: usize,
        slots: *mut Self,
        _slot_count: usize,
        frames: u32,
        channels: u32,
    ) {
        // SAFETY: the scheduler grants exclusive access to this Slot and
        // guarantees every Slot in the dependency mask is complete and
        // immutable; upstream references are shared reads of lower indices.
        let voice = unsafe { &mut *slots.add(slot_index) };
        if voice.resolved_sources.is_empty() {
            return;
        }
        voice.input.fill(0.0);
        for source in &voice.resolved_sources {
            match source {
                ResolvedRackSource::Capture => {
                    if voice.capture_len == 0 || voice.capture_channels == 0 {
                        continue;
                    }
                    // SAFETY: staged by the audio loop for this block and
                    // only read during it.
                    let capture =
                        unsafe { std::slice::from_raw_parts(voice.capture_ptr, voice.capture_len) };
                    mix_capture_into_plugin(
                        capture,
                        voice.capture_channels,
                        &mut voice.input,
                        voice.input_channels,
                        frames as usize,
                    );
                }
                ResolvedRackSource::Slot(upstream) => {
                    // SAFETY: `upstream` is a lower, completed index.
                    let upstream =
                        unsafe { &*(slots.add(*upstream) as *const RackSlotVoice<'plugin>) };
                    if upstream.process_faulted {
                        continue;
                    }
                    mix_slot_into_plugin(
                        upstream,
                        &mut voice.input,
                        voice.input_channels,
                        frames as usize,
                        channels as usize,
                    );
                }
            }
        }
    }

    fn run_single(&mut self, frames: u32, channels: u32) -> bool {
        process_rack_voice(self, frames, channels);
        // Faults are already silenced and quarantined in place; report
        // success so the scheduler does not quarantine a second time.
        true
    }

    fn run_begin(&mut self, frames: u32, _channels: u32) -> Option<u32> {
        self.output.fill(0.0);
        if self.process_faulted {
            return Some(0);
        }
        let parallel = self.parallel.as_mut()?;
        parallel
            .begin(
                &mut self.instance,
                &self.input,
                frames,
                &self.events,
                &self.parameter_events,
            )
            .ok()
    }

    fn unit_job(&mut self, unit: u32, frames: u32, channels: u32) -> UnitJob {
        self.parallel
            .as_mut()
            .expect("unit job requested for a classic Rack Slot")
            .unit_job(unit, &self.input, frames, channels)
    }

    fn run_end(&mut self, frames: u32, channels: u32, completed: u32) -> bool {
        if self.process_faulted {
            return true;
        }
        let Some(parallel) = self.parallel.as_mut() else {
            return false;
        };
        parallel
            .finish(
                &mut self.instance,
                &mut self.output,
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

/// Creates the host-owned unit instances for one activated Rack Slot when
/// the plugin declares `parallel_render_v1` and this host schedules units.
/// State and program loads are mirrored so every instance agrees on control
/// state; per-block dynamics still travel through dispatch payloads.
fn create_rack_slot_parallel_units<'plugin>(
    plugin: &'plugin LoadedPlugin,
    state: &RackSlotStateLoad,
    sample_rate_hz: u32,
    period_frames: u32,
    input_channels: u32,
    output_channels: u32,
) -> Result<Option<ParallelUnits<'plugin>>> {
    if !parallel_render::parallel_units_enabled() {
        return Ok(None);
    }
    let Some(mut units) = ParallelUnits::create(
        plugin,
        f64::from(sample_rate_hz),
        period_frames,
        input_channels,
        output_channels,
    )?
    else {
        return Ok(None);
    };
    match state {
        RackSlotStateLoad::Default => {}
        RackSlotStateLoad::Opaque(bytes) => {
            units.mirror(|instance| instance.load_state(bytes))?;
        }
        RackSlotStateLoad::LegacyPreset(preset_id) => {
            units.mirror(|instance| instance.load_preset(preset_id))?;
        }
    }
    Ok(Some(units))
}

fn process_rack_voice(voice: &mut RackSlotVoice<'_>, period_frames: u32, channels: u32) {
    voice.output.fill(0.0);
    if voice.process_faulted {
        return;
    }
    let process_result = voice.instance.process_interleaved(
        &voice.input,
        &mut voice.output,
        period_frames,
        voice.input_channels as u32,
        channels,
        &voice.events,
        &voice.parameter_events,
    );
    if let Err(error) = process_result {
        voice.output.fill(0.0);
        voice.process_faulted = true;
        eprintln!(
            "PLUGIN_PROCESS_QUARANTINED context=rack-slot:{} action=silence error={error}",
            voice.slot_id
        );
    }
}

fn mix_capture_into_plugin(
    capture: &[f32],
    capture_channels: usize,
    plugin: &mut [f32],
    plugin_channels: usize,
    frames: usize,
) {
    if capture_channels == 0 || plugin_channels == 0 {
        return;
    }
    for frame in 0..frames {
        for channel in 0..plugin_channels {
            let sample = if capture_channels == 1 {
                capture[frame]
            } else if plugin_channels == 1 {
                (capture[frame * capture_channels] + capture[frame * capture_channels + 1]) * 0.5
            } else {
                capture
                    .get(frame * capture_channels + channel)
                    .copied()
                    .unwrap_or(0.0)
            };
            plugin[frame * plugin_channels + channel] += sample;
        }
    }
}

fn mix_slot_into_plugin(
    source: &RackSlotVoice<'_>,
    plugin: &mut [f32],
    plugin_channels: usize,
    frames: usize,
    source_channels: usize,
) {
    if plugin_channels == 0 || source_channels == 0 {
        return;
    }
    let left_gain = source.level * (1.0 - source.pan.max(0.0));
    let right_gain = source.level * (1.0 + source.pan.min(0.0));
    for frame in 0..frames {
        let left = source.output[frame * source_channels] * left_gain;
        let right = source.output[frame * source_channels + 1] * right_gain;
        if plugin_channels == 1 {
            plugin[frame] += (left + right) * 0.5;
        } else {
            plugin[frame * plugin_channels] += left;
            plugin[frame * plugin_channels + 1] += right;
        }
    }
}

struct StandaloneVoice<'plugin> {
    instance_id: InstanceId,
    plugin: &'plugin LoadedPlugin,
    instance: PluginInstance<'plugin>,
    /// Host-owned unit instances for `parallel_render_v1` plugins, so PLAY
    /// mode schedules units across the same worker pool as Racks.
    parallel: Option<ParallelUnits<'plugin>>,
    input_channels: usize,
    live_parameter_target: usize,
    input: Vec<f32>,
    output: Vec<f32>,
    events: Vec<MidiEventV1>,
    parameter_events: Vec<ParameterEventV1>,
    process_faulted: bool,
}

impl<'plugin> StandaloneVoice<'plugin> {
    /// Applies one control-plane operation to the coordinator and mirrors
    /// the identical canonical input to every unit instance.
    fn mirror_control<E>(
        &mut self,
        mut operation: impl FnMut(&mut PluginInstance<'plugin>) -> Result<(), E>,
    ) -> Result<(), E> {
        operation(&mut self.instance)?;
        if let Some(parallel) = self.parallel.as_mut() {
            parallel.mirror(operation)?;
        }
        Ok(())
    }
}

/// One PLAY-mode voice as the global scheduler sees it: the same
/// begin → units → end family a Rack Slot contributes, minus cables.
//
// SAFETY: same argument as the Rack Slot implementation — instances reach
// pool workers only under the epoch protocol and unit jobs point at
// per-unit boxed cells holding isolated portable instances.
unsafe impl<'plugin> ScheduledSlot for StandaloneVoice<'plugin> {
    fn max_units(&self) -> u32 {
        self.parallel.as_ref().map_or(0, ParallelUnits::max_units)
    }

    fn run_single(&mut self, frames: u32, channels: u32) -> bool {
        self.output.fill(0.0);
        if self.process_faulted {
            return true;
        }
        self.instance
            .process_interleaved(
                &self.input,
                &mut self.output,
                frames,
                self.input_channels as u32,
                channels,
                &self.events,
                &self.parameter_events,
            )
            .is_ok()
    }

    fn run_begin(&mut self, frames: u32, _channels: u32) -> Option<u32> {
        self.output.fill(0.0);
        if self.process_faulted {
            return Some(0);
        }
        let parallel = self.parallel.as_mut()?;
        parallel
            .begin(
                &mut self.instance,
                &self.input,
                frames,
                &self.events,
                &self.parameter_events,
            )
            .ok()
    }

    fn unit_job(&mut self, unit: u32, frames: u32, channels: u32) -> UnitJob {
        self.parallel
            .as_mut()
            .expect("unit job requested for a classic standalone voice")
            .unit_job(unit, &self.input, frames, channels)
    }

    fn run_end(&mut self, frames: u32, channels: u32, completed: u32) -> bool {
        if self.process_faulted {
            return true;
        }
        let Some(parallel) = self.parallel.as_mut() else {
            return false;
        };
        parallel
            .finish(
                &mut self.instance,
                &mut self.output,
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

/// Re-prepares a PLAY voice's unit instances after playback resumes,
/// rebuilding them from the coordinator's state when an emergency stop or a
/// runtime replacement dropped them. Control paths of the audio thread only.
fn reactivate_standalone_parallel_units(
    voice: &mut StandaloneVoice<'_>,
    sample_rate: f64,
    maximum_frames: u32,
    output_channels: u32,
) -> Result<()> {
    match voice.parallel.as_mut() {
        Some(units) => units.reconfigure(
            sample_rate,
            maximum_frames,
            voice.input_channels as u32,
            output_channels,
        ),
        None => {
            if !parallel_render::parallel_units_enabled()
                || voice.plugin.parallel_layout().is_none()
            {
                return Ok(());
            }
            let state = voice.instance.save_state()?;
            voice.parallel = create_rack_slot_parallel_units(
                voice.plugin,
                &RackSlotStateLoad::Opaque(state),
                sample_rate as u32,
                maximum_frames,
                voice.input_channels as u32,
                output_channels,
            )?;
            Ok(())
        }
    }
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
        let (input_channels, output_channels) = plugin_audio_channels(plugin)?;
        if output_channels != channels as usize {
            bail!(
                "Rack Slot {} exposes {output_channels} output channels; runtime requires {channels}",
                spec.slot_id
            );
        }
        if !spec.audio_sources.is_empty() && input_channels == 0 {
            bail!(
                "Rack Slot {} has an audio input cable but plugin {} declares no audio input",
                spec.slot_id,
                spec.plugin_id
            );
        }
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
            .activate(
                f64::from(sample_rate_hz),
                period_frames,
                input_channels as u32,
                output_channels as u32,
            )
            .with_context(|| format!("activating Rack Slot {}", spec.slot_id))?;
        let parallel = create_rack_slot_parallel_units(
            plugin,
            &spec.state,
            sample_rate_hz,
            period_frames,
            input_channels as u32,
            output_channels as u32,
        )
        .with_context(|| format!("preparing parallel units for Rack Slot {}", spec.slot_id))?;
        voices.push(RackSlotVoice {
            slot_id: spec.slot_id.clone(),
            plugin,
            instance,
            parallel,
            midi_stages: spec.midi_stages.clone(),
            audio_sources: spec.audio_sources.clone(),
            resolved_sources: Vec::new(),
            deps_mask: 0,
            capture_ptr: std::ptr::null(),
            capture_len: 0,
            capture_channels: 0,
            sends_to_main: spec.sends_to_main,
            input_channels,
            level: f32::from(spec.level_per_mille) / 1_000.0,
            pan: f32::from(spec.pan_per_mille) / 1_000.0,
            input: vec![0.0; period_frames as usize * input_channels],
            output: vec![0.0; period_frames as usize * channels as usize],
            events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            parameter_events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            process_faulted: false,
        });
    }
    resolve_rack_voice_graph(&mut voices);
    Ok(voices)
}

fn rack_voices_from_prepared(
    prepared: Vec<control::PreparedRackSlot>,
) -> Vec<RackSlotVoice<'static>> {
    let mut voices = prepared
        .into_iter()
        .map(|prepared| RackSlotVoice {
            slot_id: prepared.slot_id,
            plugin: prepared.plugin,
            instance: prepared.instance.0,
            parallel: prepared.parallel.map(|units| units.0),
            midi_stages: prepared.midi_stages,
            audio_sources: prepared.audio_sources,
            resolved_sources: Vec::new(),
            deps_mask: 0,
            capture_ptr: std::ptr::null(),
            capture_len: 0,
            capture_channels: 0,
            sends_to_main: prepared.sends_to_main,
            input_channels: prepared.input_channels,
            level: f32::from(prepared.level_per_mille) / 1_000.0,
            pan: f32::from(prepared.pan_per_mille) / 1_000.0,
            input: prepared.input,
            output: prepared.output,
            events: prepared.events,
            parameter_events: prepared.parameter_events,
            process_faulted: false,
        })
        .collect::<Vec<_>>();
    resolve_rack_voice_graph(&mut voices);
    voices
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

struct AuditionLease {
    id: u64,
    instance_id: InstanceId,
    previous_sound_id: Option<String>,
}

pub struct LiveConfig {
    pub package: PathBuf,
    pub binary: Option<PathBuf>,
    pub resources: BTreeMap<String, PathBuf>,
    pub preset: Option<String>,
    pub data_root: Option<PathBuf>,
    pub audio_output: AudioOutputProfile,
    pub audio_input: Option<AudioInputProfile>,
    pub audio_state_path: PathBuf,
}

fn discover_plugin_packages(primary: &Path) -> Result<Vec<PluginPackage>> {
    let primary = PluginPackage::open(primary)?;
    let primary_id = primary.manifest().id.clone();
    let root = env::var_os("RACKFORGE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("rackforge")
        });
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
    let startup = crate::startup::StartupTimeline::new("core");
    ensure_supported_engine_profile(&config.audio_output)?;
    if let Some(input) = &config.audio_input {
        ensure_supported_input_profile(input, &config.audio_output)?;
    }
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
        persisted_parameter_links,
    ) = match checkpoint.as_ref() {
        Some(store) => match (
            store.active_mode(&session_id),
            store.active_instance_id(&session_id),
            store.selected_sound(&session_id, DEFAULT_LIVE_INSTANCE_ID),
            store.master_level(&session_id),
            store.master_pan(&session_id),
            store.live_state(&session_id),
            store.parameter_links(&session_id),
        ) {
            (
                Ok(mode),
                Ok(active_instance),
                Ok(sound_id),
                Ok(master_level),
                Ok(master_pan),
                Ok(live),
                Ok(parameter_links),
            ) => (
                mode,
                active_instance,
                sound_id,
                master_level,
                master_pan,
                live,
                parameter_links,
            ),
            (mode, active_instance, sound_id, master_level, master_pan, live, parameter_links) => {
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
                if let Err(error) = parameter_links {
                    eprintln!("SESSION_CHECKPOINT_IGNORED {error:#}");
                }
                (None, None, None, None, None, None, Vec::new())
            }
        },
        None => (None, None, None, None, None, None, Vec::new()),
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
        if !matches!(
            package.manifest().kind,
            PluginKind::Instrument | PluginKind::Effect
        ) {
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

    let live_parameter_store = LiveParameterStateStore::open(config.data_root.as_deref())?;
    let mut live_parameter_targets = Vec::with_capacity(plugins.len());
    let mut standalone_voices = Vec::with_capacity(plugins.len());
    let mut session_instances = Vec::with_capacity(plugins.len());
    for (plugin_id, plugin) in &plugins {
        let is_primary = plugin_id == &primary_id;
        let instance_id = plugin_instance_id(plugin_id, is_primary)?;
        let mut instance = plugin.create_instance()?;
        let (input_channels, plugin_output_channels) = plugin_audio_channels(plugin)?;
        if plugin_output_channels != channels {
            bail!(
                "plugin {} exposes {plugin_output_channels} output channels; Raspberry runtime currently requires {channels}",
                plugin.manifest().id
            );
        }
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
        }
        let restored_parameters: Vec<(u32, f64)> =
            live_parameter_store.restored_values(plugin_id, plugin.parameters());
        for (parameter_index, value) in restored_parameters.iter().copied() {
            crate::set_plugin_parameter(plugin, &mut instance, parameter_index, value)
                .with_context(|| {
                    format!("restoring live parameter {parameter_index} for plugin {plugin_id}")
                })?;
        }
        let live_parameter_target = live_parameter_targets.len();
        live_parameter_targets.push(LiveParameterTarget {
            plugin_id: plugin_id.clone(),
            plugin_version: plugin.manifest().version.to_string(),
            schema: plugin.parameters().clone(),
        });
        instance.activate(
            f64::from(output_rate),
            period_frames as u32,
            input_channels as u32,
            plugin_output_channels as u32,
        )?;
        session_instances.push(PluginInstanceState {
            instance_id: instance_id.clone(),
            plugin_id: plugin.manifest().id.clone(),
            plugin_name: plugin.manifest().name.clone(),
            plugin_short_name: plugin.manifest().little_short_name(),
            ui_layouts: plugin.manifest().ui_layouts.clone(),
            config_available: plugin.manifest().config_mode,
            banks: presets
                .banks
                .iter()
                .map(|bank| BankSummary {
                    id: bank.id.clone(),
                    name: bank.name.clone(),
                    order: bank.order,
                })
                .collect(),
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
                    // Deliberately not carried. A session snapshot holds
                    // every preset of every loaded plugin — six hundred and
                    // sixty-seven of them here — and the control socket that
                    // delivers it to the controller accepts sixty-four
                    // kilobytes. Tagging all of them cost twenty-one
                    // kilobytes and put the snapshot over, which left the
                    // KeyLab unable to read its own sound list.
                    //
                    // A surface that wants to know more about a sound should
                    // ask for that sound, not receive the details of six
                    // hundred it will never draw.
                    category: None,
                    tags: Vec::new(),
                    editable: preset.editable,
                })
                .collect(),
            selected_sound_id: selected.map(|preset| preset.id.clone()),
        });
        // PLAY-mode unit instances mirror the same canonical inputs the
        // coordinator just received: program, then restored parameters.
        let mut parallel = if parallel_render::parallel_units_enabled() {
            ParallelUnits::create(
                plugin,
                f64::from(output_rate),
                period_frames as u32,
                input_channels as u32,
                plugin_output_channels as u32,
            )
            .with_context(|| format!("preparing PLAY units for plugin {plugin_id}"))?
        } else {
            None
        };
        if let Some(units) = parallel.as_mut() {
            if let Some(preset) = selected {
                units
                    .mirror(|instance| instance.load_preset(&preset.id))
                    .with_context(|| format!("mirroring program for plugin {plugin_id}"))?;
            }
            for (parameter_index, value) in restored_parameters.iter().copied() {
                units
                    .mirror(|instance| instance.set_parameter(parameter_index, value))
                    .with_context(|| {
                        format!("mirroring live parameter {parameter_index} for {plugin_id}")
                    })?;
            }
        }
        standalone_voices.push(StandaloneVoice {
            instance_id,
            plugin,
            instance,
            parallel,
            input_channels,
            live_parameter_target,
            input: vec![0.0; period_frames * input_channels],
            output: vec![0.0; period_frames * channels],
            events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            parameter_events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            process_faulted: false,
        });
    }
    let live_parameter_writer =
        LiveParameterWriter::start(live_parameter_store, live_parameter_targets);
    let mut state_store = PluginStateStore::new(config.data_root.as_deref())?;
    // A first boot starts with an EMPTY library on every platform: the
    // performer builds their first Rack deliberately instead of inheriting
    // an invented one. Existing libraries load exactly as persisted.
    let mut performance_repository =
        PerformanceRepository::load_or_empty(config.data_root.as_deref())?;
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
    let mut live_state = match persisted_live {
        Some(live) if live.validate(&performance_library).is_ok() => live,
        Some(_) => {
            eprintln!("SESSION_CHECKPOINT_LIVE_IGNORED reason=library-mismatch");
            performance_repository.initial_live_state()
        }
        None => performance_repository.initial_live_state(),
    };
    // With nothing LIVE could play -- the empty first boot -- starting in
    // PLAY lands on the active instrument instead of a silent stage. A
    // persisted choice still wins.
    let initial_surface_mode = persisted_mode.unwrap_or(if live_state.active.is_some() {
        SurfaceMode::Live
    } else {
        SurfaceMode::Play
    });
    if initial_surface_mode != SurfaceMode::Live {
        live_state.deactivate();
    }
    let mut initial_rack_specs = Vec::new();
    if let Some(rack) = live_state.active.as_ref().and_then(|location| {
        performance_repository
            .library()
            .resolve_playable(location)
            .ok()
    }) {
        let compiled_slots = compile_instrument_definition(&performance_library, &rack)?;
        if compiled_slots.len() > control::MAX_ACTIVE_RACK_SLOTS {
            bail!(
                "initial Rack {} compiles to {} Slots; this engine supports at most {}",
                rack.id,
                compiled_slots.len(),
                control::MAX_ACTIVE_RACK_SLOTS
            );
        }
        for compiled in compiled_slots {
            let slot = &compiled.slot;
            let state = if let Some(reference) = &slot.state {
                RackSlotStateLoad::Opaque(state_store.read(reference)?)
            } else if let Some(program_id) = &slot.legacy_program_id {
                RackSlotStateLoad::LegacyPreset(program_id.clone())
            } else {
                RackSlotStateLoad::Default
            };
            initial_rack_specs.push(RackSlotRuntimeSpec {
                slot_id: compiled.runtime_slot_id,
                plugin_id: slot.plugin_id.clone(),
                state,
                midi_stages: compiled
                    .midi_stages
                    .iter()
                    .map(|stage| RackMidiStageRuntimeSpec {
                        transform: stage.transform.clone(),
                        keyboard_parts: stage.keyboard_parts,
                    })
                    .collect(),
                audio_sources: compiled.audio_sources.clone(),
                sends_to_main: compiled.sends_to_main,
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
    let (midi_port_names, mut midi_sources, midi_observer, connected_midi_sources) =
        connect_midi_sources(sender)?;
    let virtual_midi_source = MidiSourceKey::new(midi_port_names.len() as u32);
    let virtual_midi_source_id = MidiSourceId::new(VIRTUAL_MIDI_SOURCE_ID)?;
    midi_sources.register(
        virtual_midi_source,
        MidiSourceDescriptor {
            id: virtual_midi_source_id.clone(),
            name: "RackForge Touch Controller".into(),
            primary: false,
        },
    )?;
    connected_midi_sources
        .lock()
        .map_err(|_| anyhow::anyhow!("MIDI connection state lock poisoned"))?
        .insert(virtual_midi_source.get());
    let initial_parameter_links = compile_parameter_links_for_runtime(
        &persisted_parameter_links,
        &midi_sources,
        &standalone_voices,
        &rack_voices,
    )?;
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
    let virtual_play_route = compile_virtual_play_route(
        &midi_sources,
        &virtual_midi_source_id,
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
    let input = config
        .audio_input
        .as_ref()
        .map(|profile| open_audio_input_from_inventory(profile, &audio_devices))
        .transpose()?;
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
    if let Some(input) = &input {
        println!(
            "AUDIO_INPUT_READY id={} name={:?} backend={} rate={} channels={:?} format={:?} \
             period={} buffer={} gain_db={} nominal_buffer_ms={:.2}",
            input.device.id,
            input.device.name,
            input.device.backend_address,
            input.profile.sample_rate_hz,
            input.profile.channels,
            input.profile.sample_format,
            input.profile.period_frames,
            input.profile.buffer_frames,
            input.profile.gain_db,
            input.profile.nominal_buffer_latency_ms(),
        );
    } else {
        println!("AUDIO_INPUT_DISABLED");
    }
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
        parameter_links: persisted_parameter_links,
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
    let output_meter = Arc::new(OutputMeter::default());
    let (control_sender, control_receiver) = mpsc::sync_channel(AUDIO_CONTROL_QUEUE_CAPACITY);
    let control_path = control_socket_path();
    let control_storage = config
        .data_root
        .as_ref()
        .map(|root| crate::PluginStorage::new(root.clone()));
    let state_store = Arc::new(Mutex::new(state_store));
    let _control_server = control::start(
        &control_path,
        control::ControlServerOptions {
            store: session_store,
            audio_sender: control_sender,
            audio_state: Arc::clone(&audio_state),
            output_meter: Arc::clone(&output_meter),
            audio_state_path: config.audio_state_path,
            performance_repository: Arc::new(Mutex::new(performance_repository)),
            state_store,
            plugin_manifests: plugins
                .values()
                .map(|plugin| (plugin.manifest().id.clone(), plugin.manifest().clone()))
                .collect(),
            portable_plugins: plugins
                .values()
                .filter_map(|plugin| {
                    control::PortableControlPlugin::new(plugin)
                        .map(|runtime| (plugin.manifest().id.clone(), runtime))
                })
                .collect(),
            midi_sources: midi_sources.clone(),
            midi_observer,
            connected_midi_sources: Arc::clone(&connected_midi_sources),
            plugin_sample_rate: f64::from(output_rate),
            plugin_maximum_frames: period_frames as u32,
            plugin_output_channels: channels as u32,
            storage: control_storage,
            checkpoint,
        },
    )?;
    println!("CONTROL_READY socket={}", control_path.display());
    audio_loop(AudioLoopContext {
        initial_output: output,
        initial_input: input,
        receiver: &receiver,
        control_receiver: &control_receiver,
        plugins: &plugins,
        standalone_voices: &mut standalone_voices,
        active_instance_id,
        rack_voices,
        parameter_links: initial_parameter_links,
        play_route: &play_route,
        virtual_play_route: &virtual_play_route,
        virtual_midi_source,
        midi_source_count: midi_port_names.len() + 1,
        initial_master_level,
        initial_master_pan,
        render_mode: resolve_render_mode(initial_surface_mode, initial_rack_specs.len()),
        audio_state,
        output_meter,
        live_parameter_writer: live_parameter_writer.handle(),
        startup,
    })
}

fn control_socket_path() -> PathBuf {
    env::var_os("RACKFORGE_CONTROL_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let root = env::var_os("RACKFORGE_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("rackforge")
                });
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
type ConnectedMidiSources = (
    Vec<String>,
    MidiSourceRegistry,
    Receiver<IngressMidiEvent>,
    Arc<Mutex<BTreeSet<u32>>>,
);

fn connect_midi_sources(sender: SyncSender<IngressMidiEvent>) -> Result<ConnectedMidiSources> {
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
    let (observer_sender, observer_receiver) = mpsc::sync_channel(64);
    let connected_sources = Arc::new(Mutex::new(BTreeSet::new()));
    midi_hotplug::spawn(
        sender,
        Some(observer_sender),
        Arc::clone(&connected_sources),
        supervised,
        midi_hotplug::DEFAULT_POLL_INTERVAL,
    )?;
    Ok((names, registry, observer_receiver, connected_sources))
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

fn compile_virtual_play_route(
    sources: &MidiSourceRegistry,
    source_id: &MidiSourceId,
    channel_model: PluginChannelModel,
) -> Result<CompiledMidiRoute> {
    let matches = MidiRouteMatch {
        source: MidiSourceSelector::Source {
            source_id: source_id.clone(),
        },
        ..Default::default()
    };
    MidiRoute {
        schema_version: MIDI_ROUTING_SCHEMA_VERSION,
        id: MidiRouteId::new("play.touch")?,
        enabled: true,
        matches,
        transform: MidiRouteTransform::default(),
        target: MidiRouteTarget {
            instance_id: MidiTargetId::new(DEFAULT_LIVE_INSTANCE_ID)?,
            input_bus_id: MidiInputBusId::new(DEFAULT_INPUT_BUS_ID)?,
        },
    }
    .compile(sources, channel_model)
    .map_err(Into::into)
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

fn ensure_supported_input_profile(
    input: &AudioInputProfile,
    output: &AudioOutputProfile,
) -> Result<()> {
    input.validate()?;
    if input.sample_format != AudioSampleFormat::S32Le {
        bail!(
            "audio engine currently captures S32_LE only, requested {:?}",
            input.sample_format
        );
    }
    if input.sample_rate_hz != output.sample_rate_hz || input.period_frames != output.period_frames
    {
        bail!(
            "audio input and output must share sample rate and period (input {} Hz/{} frames, output {} Hz/{} frames)",
            input.sample_rate_hz,
            input.period_frames,
            output.sample_rate_hz,
            output.period_frames
        );
    }
    Ok(())
}

fn plugin_audio_channels(plugin: &LoadedPlugin) -> Result<(usize, usize)> {
    let audio = plugin.manifest().resolved_audio_contract();
    let input_channels = audio.input_channels() as usize;
    let output_channels = audio.output_channels() as usize;
    if input_channels > rackforge_audio_api::MAX_ACTIVE_INPUT_CHANNELS {
        bail!(
            "plugin {} exposes {input_channels} input channels; this runtime supports at most {}",
            plugin.manifest().id,
            rackforge_audio_api::MAX_ACTIVE_INPUT_CHANNELS
        );
    }
    if output_channels == 0 {
        bail!("plugin {} exposes no audio output", plugin.manifest().id);
    }
    Ok((input_channels, output_channels))
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

fn compile_parameter_links_for_runtime(
    links: &[ParameterLink],
    sources: &MidiSourceRegistry,
    standalone_voices: &[StandaloneVoice<'_>],
    rack_voices: &[RackSlotVoice<'_>],
) -> Result<Vec<CompiledParameterLink>> {
    links
        .iter()
        .filter_map(|link| {
            // Missing hardware remains persisted and pending. It is never
            // treated as an invalid link merely because it is unplugged.
            let source_key = sources.resolve_optional(&link.source.source_id)?;
            let schema = standalone_voices
                .iter()
                .find(|voice| voice.instance_id.as_str() == link.instance_id)
                .map(|voice| voice.plugin.parameters())
                .or_else(|| {
                    rack_voices
                        .iter()
                        .find(|voice| voice.slot_id == link.instance_id)
                        .map(|voice| voice.plugin.parameters())
                });
            Some(match schema {
                Some(schema) => CompiledParameterLink::new(link.clone(), source_key, schema),
                None => Err(anyhow::anyhow!(
                    "parameter link {} targets unknown instance {}",
                    link.id,
                    link.instance_id
                )),
            })
        })
        .collect()
}

fn apply_parameter_links(
    links: &[CompiledParameterLink],
    event: IngressMidiEvent,
    instance_id: &str,
    output: &mut Vec<ParameterEventV1>,
) -> bool {
    let mut consume = false;
    for link in links
        .iter()
        .filter(|link| link.link.instance_id == instance_id)
    {
        let Some(mapped) = link.apply(event) else {
            continue;
        };
        consume |= mapped.pass_through == ParameterLinkPassThrough::Consume;
        if output.len() < MAX_EVENTS_PER_BLOCK {
            output.push(mapped.event);
        }
    }
    consume
}

struct AudioLoopContext<'a> {
    initial_output: OpenedAudioOutput,
    initial_input: Option<OpenedAudioInput>,
    receiver: &'a Receiver<IngressMidiEvent>,
    control_receiver: &'a Receiver<AudioControlCommand>,
    plugins: &'a BTreeMap<String, &'static LoadedPlugin>,
    standalone_voices: &'a mut [StandaloneVoice<'static>],
    active_instance_id: InstanceId,
    rack_voices: Vec<RackSlotVoice<'static>>,
    parameter_links: Vec<CompiledParameterLink>,
    play_route: &'a CompiledMidiRoute,
    virtual_play_route: &'a CompiledMidiRoute,
    virtual_midi_source: MidiSourceKey,
    midi_source_count: usize,
    initial_master_level: MasterLevel,
    initial_master_pan: MasterPan,
    render_mode: AudioRenderMode,
    audio_state: Arc<Mutex<AudioOutputState>>,
    output_meter: Arc<OutputMeter>,
    live_parameter_writer: LiveParameterWriterHandle,
    startup: crate::startup::StartupTimeline,
}

fn audio_loop(context: AudioLoopContext<'_>) -> Result<()> {
    let AudioLoopContext {
        initial_output,
        initial_input,
        receiver,
        control_receiver,
        plugins,
        standalone_voices,
        mut active_instance_id,
        mut rack_voices,
        mut parameter_links,
        play_route,
        virtual_play_route,
        virtual_midi_source,
        midi_source_count,
        initial_master_level,
        initial_master_pan,
        mut render_mode,
        audio_state,
        output_meter,
        live_parameter_writer,
        startup,
    } = context;
    let mut output = Some(initial_output);
    let mut input = initial_input;
    let mut period_frames = output.as_ref().unwrap().profile.period_frames as usize;
    let mut channels = output.as_ref().unwrap().profile.channels as usize;
    let mut output_rate = output.as_ref().unwrap().profile.sample_rate_hz as usize;
    let capture_channels = input
        .as_ref()
        .map_or(0, |capture| capture.profile.channels.len());
    let capture_stream_channels = input
        .as_ref()
        .map_or(0, |capture| capture.profile.stream_channels() as usize);
    let capture_gain = input.as_ref().map_or(1.0, |capture| {
        10.0_f32.powf(f32::from(capture.profile.gain_db) / 20.0)
    });
    let capture_indices = input
        .as_ref()
        .map(|capture| {
            capture
                .profile
                .channels
                .iter()
                .map(|channel| *channel as usize - 1)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut device_input = vec![0_i32; period_frames * capture_stream_channels];
    let mut captured_input = vec![0.0_f32; period_frames * capture_channels];
    let mut plugin_input =
        vec![0.0_f32; period_frames * rackforge_audio_api::MAX_ACTIVE_INPUT_CHANNELS];
    let mut plugin_output = vec![0.0_f32; period_frames * channels];
    let mut mix_output = vec![0.0_f32; period_frames * channels];
    let mut device_output = vec![0_i32; period_frames * channels];
    let mut events = Vec::with_capacity(MAX_EVENTS_PER_BLOCK);
    let mut sequencer_events: Vec<MidiEventV1> = Vec::with_capacity(MAX_EVENTS_PER_BLOCK);
    let mut parameter_events = Vec::with_capacity(MAX_EVENTS_PER_BLOCK);
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
    let mut pending_virtual_midi = Vec::with_capacity(32);
    // The host sequencer: transport and lanes, advanced once per period so
    // pattern MIDI joins the block sample-accurately. Rack-mode distribution
    // across Slot filters is the next stage; today the stream reaches the
    // standalone voice the way the desktop host's does.
    let mut sequencer = crate::sequencer::SequencerEngine::new(output_rate as f64)
        .or_else(|| crate::sequencer::SequencerEngine::new(48_000.0))
        .expect("48 kHz is inside the transport bounds");
    let (retired_sender, retired_receiver) = mpsc::sync_channel::<RetiredAudioRuntime>(16);
    let _retired_reclaimer = thread::Builder::new()
        .name("rackforge-live-voice-reclaimer".into())
        .spawn(move || {
            while let Ok(retired) = retired_receiver.recv() {
                match retired {
                    RetiredAudioRuntime::Standalone(instance) => drop(instance),
                    RetiredAudioRuntime::PortableRack(voices) => drop(voices.0),
                }
            }
        })?;
    let mut deferred_retire = Vec::with_capacity(16);
    let mut startup_ready = false;

    // Engaged here, not during setup: `SCHED_FIFO` is a per-thread property and
    // this is the thread that runs the audio loop. Setup work stays on the
    // ordinary scheduler, where blocking on the filesystem is harmless.
    let realtime_status = realtime::engage(realtime::DEFAULT_AUDIO_PRIORITY);
    println!("{realtime_status}");
    if let Some(remedy) = realtime_status.remedy() {
        eprintln!("REALTIME_REMEDY {remedy}");
    }
    let render_telemetry = RenderTelemetry::new(parallel_render::MAX_RENDER_SLOTS);
    spawn_telemetry_publisher(&render_telemetry, Duration::from_secs(1));
    let mut rack_renderer = RenderPool::automatic(Arc::clone(&render_telemetry));
    render_telemetry.set_slot_labels(
        rack_voices
            .iter()
            .map(|voice| voice.slot_id.clone())
            .collect(),
    );
    let mut xruns = XrunMonitor::new(output_rate as u32, period_frames);
    let mut input_xruns = XrunMonitor::new(output_rate as u32, period_frames);

    loop {
        while let Some(instance) = deferred_retire.pop() {
            match retired_sender.try_send(instance) {
                Ok(()) => {}
                Err(TrySendError::Full(instance)) | Err(TrySendError::Disconnected(instance)) => {
                    deferred_retire.push(instance);
                    break;
                }
            }
        }
        while let Ok(command) = control_receiver.try_recv() {
            match command {
                AudioControlCommand::InjectVirtualMidi { events } => {
                    let available = MAX_EVENTS_PER_BLOCK.saturating_sub(pending_virtual_midi.len());
                    let accepted = events.len().min(available);
                    let omitted = events.len().saturating_sub(accepted);
                    pending_virtual_midi.extend(events.into_iter().take(accepted));
                    dropped_events += omitted;
                }
                AudioControlCommand::Sequencer { command, reply } => {
                    let _ = reply.try_send(sequencer.apply(&command));
                }
                AudioControlCommand::SequencerStatus { reply } => {
                    let _ = reply.try_send(Ok(sequencer.status()));
                }
                AudioControlCommand::ApplyAudioOutput { profile, reply } => {
                    let result = if input.as_ref().is_some_and(|capture| {
                        capture.profile.sample_rate_hz != profile.sample_rate_hz
                            || capture.profile.period_frames != profile.period_frames
                    }) {
                        Err(anyhow::anyhow!(
                            "audio output rate/period cannot change while capture is active; disable or reconfigure the input first"
                        ))
                    } else {
                        reconfigure_audio_output(
                            &mut output,
                            standalone_voices,
                            &mut rack_voices,
                            profile,
                            &audio_state,
                        )
                    };
                    if let Ok(snapshot) = &result {
                        period_frames = snapshot.active_profile.period_frames as usize;
                        channels = snapshot.active_profile.channels as usize;
                        output_rate = snapshot.active_profile.sample_rate_hz as usize;
                        xruns.reconfigure(output_rate as u32, period_frames);
                        plugin_output.resize(period_frames * channels, 0.0);
                        mix_output.resize(period_frames * channels, 0.0);
                        for voice in &mut rack_voices {
                            voice
                                .input
                                .resize(period_frames * voice.input_channels, 0.0);
                            voice.output.resize(period_frames * channels, 0.0);
                        }
                        for voice in standalone_voices.iter_mut() {
                            voice
                                .input
                                .resize(period_frames * voice.input_channels, 0.0);
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
                    if mode != SurfaceMode::Live && !rack_voices.is_empty() {
                        let released = rack_voices.len();
                        events.clear();
                        let retired = std::mem::take(&mut rack_voices);
                        if let Err(mut native_voices) =
                            retire_portable_rack(retired, &mut deferred_retire)
                        {
                            for voice in &mut native_voices {
                                voice.events.clear();
                                if let Err(error) = voice.instance.reset() {
                                    eprintln!(
                                        "LIVE_RACK_RELEASE_RESET_FAILED slot={} error={error:#}",
                                        voice.slot_id
                                    );
                                }
                            }
                        }
                        println!("LIVE_RACK_RELEASED reason=surface-mode-change voices={released}");
                    }
                    let requested_mode = resolve_render_mode(mode, rack_voices.len());
                    if mode == SurfaceMode::Live && requested_mode == AudioRenderMode::Silent {
                        println!(
                            "AUDIO_RENDER_MODE_RECONCILED requested=Live actual=Silent reason=no-rack-voices"
                        );
                    }
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
                            .mirror_control(|instance| instance.reset())
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
                                .mirror_control(|instance| instance.load_preset(&sound_id))
                                .map_err(|error| error.to_string())?;
                            voice.process_faulted = false;
                            live_parameter_writer.clear(voice.live_parameter_target);
                            Ok(())
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
                                .mirror_control(|instance| instance.load_state(&bytes))
                                .map_err(|error| error.to_string())?;
                            voice.process_faulted = false;
                            Ok(())
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
                                .mirror_control(|instance| {
                                    instance.set_parameter(parameter_index, value)
                                })
                                .map_err(|error| error.to_string())?;
                            let canonical = voice
                                .instance
                                .get_parameter(parameter_index)
                                .map_err(|error| error.to_string())?;
                            live_parameter_writer.try_record(
                                voice.live_parameter_target,
                                parameter_index,
                                canonical,
                            );
                            Ok(canonical)
                        });
                    let _ = reply.send(result);
                }
                AudioControlCommand::ReplaceParameterLinks { links, reply } => {
                    parameter_links = links;
                    let _ = reply.send(Ok(()));
                }
                AudioControlCommand::ReplaceStandaloneVoice {
                    instance_id,
                    instance,
                    reply,
                } => {
                    let result =
                        standalone_voice_mut(standalone_voices, &instance_id).map(|voice| {
                            let retired = std::mem::replace(&mut voice.instance, instance.0);
                            deferred_retire.push(RetiredAudioRuntime::Standalone(
                                control::PreparedPluginInstance(retired),
                            ));
                            // The old units mirrored the retired runtime;
                            // rebuild them from the replacement's state.
                            voice.parallel = None;
                            voice.process_faulted = false;
                            if let Err(error) = reactivate_standalone_parallel_units(
                                voice,
                                output_rate as f64,
                                period_frames as u32,
                                channels as u32,
                            ) {
                                eprintln!(
                                    "PLAY_UNITS_REBUILD_FAILED instance={instance_id} \
                                     error={error:#}"
                                );
                            }
                        });
                    let _ = reply.send(result);
                }
                AudioControlCommand::ActivateRack {
                    rack_id,
                    instance_id,
                    slots,
                    prepared_slots,
                    reply,
                } => {
                    let result = match prepared_slots {
                        Some(prepared) => Ok(rack_voices_from_prepared(prepared)),
                        None => create_rack_voices(
                            plugins,
                            &slots,
                            output_rate as u32,
                            period_frames as u32,
                            channels as u32,
                        ),
                    }
                    .map(|voices| {
                        let retired = std::mem::replace(&mut rack_voices, voices);
                        if let Err(native_voices) =
                            retire_portable_rack(retired, &mut deferred_retire)
                        {
                            // Native ABI instances do not yet promise safe
                            // cross-thread destruction, so retain the previous
                            // behavior for that compatibility path.
                            drop(native_voices);
                        }
                        render_mode = AudioRenderMode::Rack;
                        render_telemetry.set_slot_labels(
                            rack_voices
                                .iter()
                                .map(|voice| voice.slot_id.clone())
                                .collect(),
                        );
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
                            .mirror_control(|instance| instance.reset())
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
                                    restore_after_audition(voice, &lease)
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
                        let voice = standalone_voice_mut(standalone_voices, &instance_id)?;
                        let prepared = voice
                            .instance
                            .begin_program_edit(&request)
                            .map_err(|error| error.to_string())?;
                        let editor = voice
                            .instance
                            .program_editor_view(&prepared.document)
                            .map_err(|error| error.to_string())?;
                        voice
                            .mirror_control(|instance| instance.reset())
                            .map_err(|error| error.to_string())?;
                        if voice
                            .instance
                            .preview_program(&prepared)
                            .map_err(|error| error.to_string())?
                        {
                            if let Some(parallel) = voice.parallel.as_mut() {
                                parallel
                                    .mirror(|instance| {
                                        instance.preview_program(&prepared).map(|_| ())
                                    })
                                    .map_err(|error| error.to_string())?;
                            }
                        } else {
                            voice
                                .mirror_control(|instance| {
                                    instance.load_preset(&prepared.preview_sound_id)
                                })
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
                        let voice = standalone_voice_mut(standalone_voices, &instance_id)?;
                        let prepared = voice
                            .instance
                            .prepare_program_save(&document)
                            .map_err(|error| error.to_string())?;
                        let editor = voice
                            .instance
                            .program_editor_view(&prepared.document)
                            .map_err(|error| error.to_string())?;
                        if voice
                            .instance
                            .preview_program(&prepared)
                            .map_err(|error| error.to_string())?
                        {
                            if let Some(parallel) = voice.parallel.as_mut() {
                                parallel
                                    .mirror(|instance| {
                                        instance.preview_program(&prepared).map(|_| ())
                                    })
                                    .map_err(|error| error.to_string())?;
                            }
                        } else {
                            voice
                                .mirror_control(|instance| {
                                    instance.load_preset(&prepared.preview_sound_id)
                                })
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
                        let voice = standalone_voice_mut(standalone_voices, &instance_id)?;
                        let prepared = voice
                            .instance
                            .apply_program_edit(&request)
                            .map_err(|error| error.to_string())?;
                        let editor = voice
                            .instance
                            .program_editor_view(&prepared.document)
                            .map_err(|error| error.to_string())?;
                        if voice
                            .instance
                            .preview_program(&prepared)
                            .map_err(|error| error.to_string())?
                        {
                            if let Some(parallel) = voice.parallel.as_mut() {
                                parallel
                                    .mirror(|instance| {
                                        instance.preview_program(&prepared).map(|_| ())
                                    })
                                    .map_err(|error| error.to_string())?;
                            }
                        } else {
                            voice
                                .mirror_control(|instance| {
                                    instance.load_preset(&prepared.preview_sound_id)
                                })
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
                                .mirror_control(|instance| instance.install_program(&prepared))
                                .map_err(|error| error.to_string())?;
                            voice
                                .instance
                                .preset_catalog()
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
        parameter_events.clear();
        for voice in &mut rack_voices {
            voice.events.clear();
            voice.parameter_events.clear();
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
                        replay_rack_controller_state(&controller_states, play_route, voice)
                    })
                    .sum(),
            };
            dropped_events += omitted;
            if omitted > 0 {
                eprintln!("MIDI_CONTROLLER_REPLAY_TRUNCATED omitted={omitted}");
            }
            replay_controller_state = false;
        }
        for event in pending_virtual_midi.drain(..) {
            let packet = event.packet;
            controller_states.observe(event.source, plugin_midi_event(packet));
            if event.source != virtual_midi_source {
                if reserved_midi_controls.consume(event.source, plugin_midi_event(packet)) {
                    continue;
                }
                match render_mode {
                    AudioRenderMode::Silent => {}
                    AudioRenderMode::Plugin => {
                        let parameter_start = parameter_events.len();
                        let consume = apply_parameter_links(
                            &parameter_links,
                            event,
                            active_instance_id.as_str(),
                            &mut parameter_events,
                        );
                        if let Some(voice) = standalone_voices
                            .iter()
                            .find(|voice| voice.instance_id == active_instance_id)
                        {
                            for mapped in &parameter_events[parameter_start..] {
                                live_parameter_writer.try_record(
                                    voice.live_parameter_target,
                                    mapped.parameter_index,
                                    mapped.value,
                                );
                            }
                        }
                        if !consume && let Some(routed) = play_route.route(event) {
                            if events.len() < MAX_EVENTS_PER_BLOCK {
                                events.push(plugin_midi_event(routed.packet));
                            } else {
                                dropped_events += 1;
                            }
                        }
                    }
                    AudioRenderMode::Rack => {
                        for voice in &mut rack_voices {
                            let consume = apply_parameter_links(
                                &parameter_links,
                                event,
                                &voice.slot_id,
                                &mut voice.parameter_events,
                            );
                            if !consume
                                && let Some(routed) = route_rack_event_through_stages(
                                    event,
                                    &voice.midi_stages,
                                    play_route,
                                )
                            {
                                if voice.events.len() < MAX_EVENTS_PER_BLOCK {
                                    voice.events.push(routed);
                                } else {
                                    dropped_events += 1;
                                }
                            }
                        }
                    }
                }
                continue;
            }
            match render_mode {
                AudioRenderMode::Silent => {}
                AudioRenderMode::Plugin => {
                    if let Some(routed) = virtual_play_route.route(event) {
                        if events.len() < MAX_EVENTS_PER_BLOCK {
                            events.push(plugin_midi_event(routed.packet));
                        } else {
                            dropped_events += 1;
                        }
                    }
                }
                AudioRenderMode::Rack => {
                    for voice in &mut rack_voices {
                        if let Some(routed) = route_rack_event_through_stages(
                            event,
                            &voice.midi_stages,
                            virtual_play_route,
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
        while let Ok(event) = receiver.try_recv() {
            let plugin_event = plugin_midi_event(event.packet);
            if reserved_midi_controls.consume(event.source, plugin_event) {
                continue;
            }
            controller_states.observe(event.source, plugin_event);
            match render_mode {
                AudioRenderMode::Silent => {}
                AudioRenderMode::Plugin => {
                    let parameter_start = parameter_events.len();
                    let consume = apply_parameter_links(
                        &parameter_links,
                        event,
                        active_instance_id.as_str(),
                        &mut parameter_events,
                    );
                    if let Some(voice) = standalone_voices
                        .iter()
                        .find(|voice| voice.instance_id == active_instance_id)
                    {
                        for mapped in &parameter_events[parameter_start..] {
                            live_parameter_writer.try_record(
                                voice.live_parameter_target,
                                mapped.parameter_index,
                                mapped.value,
                            );
                        }
                    }
                    if !consume && let Some(routed) = play_route.route(event) {
                        if events.len() < MAX_EVENTS_PER_BLOCK {
                            events.push(plugin_midi_event(routed.packet));
                        } else {
                            dropped_events += 1;
                        }
                    }
                }
                AudioRenderMode::Rack => {
                    for voice in &mut rack_voices {
                        let consume = apply_parameter_links(
                            &parameter_links,
                            event,
                            &voice.slot_id,
                            &mut voice.parameter_events,
                        );
                        if !consume
                            && let Some(routed) = route_rack_event_through_stages(
                                event,
                                &voice.midi_stages,
                                play_route,
                            )
                        {
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

        captured_input.fill(0.0);
        if let Some(capture) = input.as_mut() {
            let io = capture.pcm.io_i32()?;
            read_period(
                &capture.pcm,
                &io,
                &mut device_input,
                period_frames,
                capture_stream_channels,
                &mut input_xruns,
            )?;
            map_capture_channels(
                &device_input,
                capture_stream_channels,
                &capture_indices,
                capture_gain,
                &mut captured_input,
            );
            if let Some(report) = input_xruns.tick() {
                eprintln!("AUDIO_INPUT_{report}");
            }
        }

        // The sequencer advances whether or not anything is listening: the
        // transport is the machine's clock, not the instrument's.
        //
        // Each lane speaks on its own wire channel, so in Rack mode its
        // events enter exactly where the machine's own keyboard enters — the
        // virtual source, through every Slot's stage router — and the Rack's
        // existing channel filters, key ranges, transposes and velocity
        // curves decide who hears which lane. Frame offsets ride the packet
        // the whole way: quantised launches stay sample-accurate per Slot.
        sequencer_events.clear();
        sequencer.render_block(period_frames as u32, &mut sequencer_events);
        match render_mode {
            AudioRenderMode::Silent => {}
            AudioRenderMode::Plugin => {
                for event in &sequencer_events {
                    if events.len() < MAX_EVENTS_PER_BLOCK {
                        events.push(*event);
                    } else {
                        dropped_events += 1;
                    }
                }
            }
            AudioRenderMode::Rack => {
                for event in &sequencer_events {
                    let ingress = IngressMidiEvent {
                        source: virtual_midi_source,
                        packet: MidiPacket {
                            frame: event.frame,
                            length: event.length,
                            data: event.data,
                        },
                    };
                    for voice in &mut rack_voices {
                        if let Some(routed) = route_rack_event_through_stages(
                            ingress,
                            &voice.midi_stages,
                            virtual_play_route,
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
                let deadline_ns =
                    period_frames as u64 * 1_000_000_000 / (output_rate as u64).max(1);
                let voice = standalone_voice_mut(standalone_voices, &active_instance_id)
                    .map_err(anyhow::Error::msg)?;
                prepare_plugin_capture(
                    &captured_input,
                    capture_channels,
                    &mut plugin_input,
                    voice.input_channels,
                    period_frames,
                );
                voice
                    .input
                    .copy_from_slice(&plugin_input[..period_frames * voice.input_channels]);
                voice.events.clear();
                voice.events.extend_from_slice(&events);
                voice.parameter_events.clear();
                voice.parameter_events.extend_from_slice(&parameter_events);
                let was_faulted = voice.process_faulted;
                let render_started = Instant::now();
                let scheduled = rack_renderer.process(
                    std::slice::from_mut(voice),
                    period_frames as u32,
                    channels as u32,
                    deadline_ns,
                );
                if !scheduled {
                    process_slots_sequential(
                        std::slice::from_mut(voice),
                        period_frames as u32,
                        channels as u32,
                        &render_telemetry,
                    );
                    render_telemetry.record_block(
                        render_started.elapsed().as_nanos() as u64,
                        deadline_ns,
                        None,
                    );
                }
                if voice.process_faulted {
                    if !was_faulted {
                        eprintln!(
                            "PLUGIN_PROCESS_QUARANTINED context=standalone:{active_instance_id} \
                             action=silence"
                        );
                    }
                    render_mode = AudioRenderMode::Silent;
                } else {
                    mix_output.copy_from_slice(&voice.output);
                }
            }
            AudioRenderMode::Rack => {
                let deadline_ns =
                    period_frames as u64 * 1_000_000_000 / (output_rate as u64).max(1);
                // Stage this block's capture for the Slots whose cables read
                // the hardware input; the gather step consumes it.
                for voice in &mut rack_voices {
                    voice.capture_ptr = captured_input.as_ptr();
                    voice.capture_len = captured_input.len();
                    voice.capture_channels = capture_channels;
                }
                let render_started = Instant::now();
                let scheduled = if rack_renderer.process(
                    &mut rack_voices,
                    period_frames as u32,
                    channels as u32,
                    deadline_ns,
                ) {
                    true
                } else {
                    process_slots_sequential(
                        &mut rack_voices,
                        period_frames as u32,
                        channels as u32,
                        &render_telemetry,
                    );
                    false
                };
                if !scheduled {
                    render_telemetry.record_block(
                        render_started.elapsed().as_nanos() as u64,
                        deadline_ns,
                        None,
                    );
                }
                for voice in &rack_voices {
                    if voice.process_faulted || !voice.sends_to_main {
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
        let mut block_left_peak = 0.0_f32;
        let mut block_right_peak = 0.0_f32;
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
                if channel == 0 {
                    block_left_peak = block_left_peak.max(mastered.abs());
                } else if channel == 1 {
                    block_right_peak = block_right_peak.max(mastered.abs());
                }
                meter_peak = meter_peak.max(mastered.abs());
                meter_clipped += usize::from(mastered.abs() > 0.95);
                *target = (mastered.clamp(-0.95, 0.95) * i32::MAX as f32) as i32;
            }
        }
        if channels == 1 {
            block_right_peak = block_left_peak;
        }
        output_meter.observe_stereo(block_left_peak, block_right_peak);
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
        if !startup_ready {
            startup_ready = true;
            startup.advance(crate::startup::StartupPhase::AudioReady)?;
            println!("READY_TO_PLAY");
            if let Err(error) = crate::startup::notify_service_ready(
                "RackForge audio rendered its first device period",
            ) {
                eprintln!("SYSTEMD_READY_FAILED error={error}");
            }
        }
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

#[cfg(test)]
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
        // Rebuilt from the coordinator's state when playback resumes.
        voice.parallel = None;
        voice.events.clear();
        voice.output.fill(0.0);
        voice.process_faulted = false;
    }
    for voice in rack_voices {
        if let Err(error) = replace_with_stopped_runtime(voice.plugin, &mut voice.instance) {
            failures.push(format!("slot {}: {error}", voice.slot_id));
        }
        // Unit instances are dropped with the stopped runtime and rebuilt
        // from the coordinator's state when playback resumes.
        voice.parallel = None;
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
        AudioRenderMode::Plugin => {
            let voice = standalone_voice_mut(standalone_voices, active_instance_id)?;
            voice
                .instance
                .activate(
                    sample_rate,
                    maximum_frames,
                    voice.input_channels as u32,
                    output_channels,
                )
                .and_then(|()| {
                    reactivate_standalone_parallel_units(
                        voice,
                        sample_rate,
                        maximum_frames,
                        output_channels,
                    )
                })
                .map(|()| {
                    voice.process_faulted = false;
                })
                .map_err(|error| error.to_string())
        }
        AudioRenderMode::Rack => {
            for index in 0..rack_voices.len() {
                let activation = {
                    let voice = &mut rack_voices[index];
                    voice
                        .instance
                        .activate(
                            sample_rate,
                            maximum_frames,
                            voice.input_channels as u32,
                            output_channels,
                        )
                        .and_then(|()| {
                            reactivate_rack_slot_parallel_units(
                                voice,
                                sample_rate,
                                maximum_frames,
                                output_channels,
                            )
                        })
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

/// Re-prepares a Slot's unit instances after playback resumes, rebuilding
/// them from the coordinator's state when an emergency stop dropped them.
/// This runs on control paths of the audio thread, never inside a deadline.
fn reactivate_rack_slot_parallel_units(
    voice: &mut RackSlotVoice<'_>,
    sample_rate: f64,
    maximum_frames: u32,
    output_channels: u32,
) -> Result<()> {
    match voice.parallel.as_mut() {
        Some(units) => units.reconfigure(
            sample_rate,
            maximum_frames,
            voice.input_channels as u32,
            output_channels,
        ),
        None => {
            if !parallel_render::parallel_units_enabled()
                || voice.plugin.parallel_layout().is_none()
            {
                return Ok(());
            }
            let state = voice.instance.save_state()?;
            voice.parallel = create_rack_slot_parallel_units(
                voice.plugin,
                &RackSlotStateLoad::Opaque(state),
                sample_rate as u32,
                maximum_frames,
                voice.input_channels as u32,
                output_channels,
            )?;
            Ok(())
        }
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
            .mirror_control(|instance| instance.deactivate())
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
                    voice.input_channels as u32,
                    requested.channels,
                )
                .with_context(|| format!("activating plugin instance {}", voice.instance_id))?;
            if let Some(units) = voice.parallel.as_mut() {
                units
                    .reconfigure(
                        f64::from(requested.sample_rate_hz),
                        requested.period_frames,
                        voice.input_channels as u32,
                        requested.channels,
                    )
                    .with_context(|| {
                        format!("activating PLAY units for instance {}", voice.instance_id)
                    })?;
            }
        }
        for voice in rack_voices.iter_mut() {
            voice
                .instance
                .activate(
                    f64::from(requested.sample_rate_hz),
                    requested.period_frames,
                    voice.input_channels as u32,
                    requested.channels,
                )
                .with_context(|| format!("activating Rack Slot {}", voice.slot_id))?;
            if let Some(units) = voice.parallel.as_mut() {
                units
                    .reconfigure(
                        f64::from(requested.sample_rate_hz),
                        requested.period_frames,
                        voice.input_channels as u32,
                        requested.channels,
                    )
                    .with_context(|| {
                        format!("activating parallel units for Rack Slot {}", voice.slot_id)
                    })?;
            }
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
                        voice.input_channels as u32,
                        previous_profile.channels,
                    )?;
                    if let Some(units) = voice.parallel.as_mut() {
                        units.reconfigure(
                            f64::from(previous_profile.sample_rate_hz),
                            previous_profile.period_frames,
                            voice.input_channels as u32,
                            previous_profile.channels,
                        )?;
                    }
                }
                for voice in rack_voices.iter_mut() {
                    voice.instance.activate(
                        f64::from(previous_profile.sample_rate_hz),
                        previous_profile.period_frames,
                        voice.input_channels as u32,
                        previous_profile.channels,
                    )?;
                    if let Some(units) = voice.parallel.as_mut() {
                        units.reconfigure(
                            f64::from(previous_profile.sample_rate_hz),
                            previous_profile.period_frames,
                            voice.input_channels as u32,
                            previous_profile.channels,
                        )?;
                    }
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

#[cfg(test)]
fn route_rack_event_transformed(
    event: IngressMidiEvent,
    transform: &rackforge_performance_api::RackMidiTransform,
    keyboard_parts: Option<RackKeyboardParts>,
    play_route: &CompiledMidiRoute,
) -> Option<MidiEventV1> {
    let stage = RackMidiStageRuntimeSpec {
        transform: transform.clone(),
        keyboard_parts,
    };
    route_rack_event_through_stages(event, std::slice::from_ref(&stage), play_route)
}

/// Applies graph MIDI cables in their actual nesting order. The first stage
/// sees the physical/controller event, then each child Rack receives the
/// packet produced by its parent. No allocation or transform approximation is
/// performed on the realtime path.
fn route_rack_event_through_stages(
    event: IngressMidiEvent,
    stages: &[RackMidiStageRuntimeSpec],
    play_route: &CompiledMidiRoute,
) -> Option<MidiEventV1> {
    if stages.is_empty() {
        return play_route
            .route(event)
            .map(|routed| plugin_midi_event(routed.packet));
    }

    let mut packet = event.packet;
    for (index, stage) in stages.iter().enumerate() {
        let transform = &stage.transform;
        let status = packet.data[0] & 0xf0;
        if transform.notes_only && !matches!(status, 0x80 | 0x90) {
            return None;
        }
        let keyed_message = matches!(status, 0x80 | 0x90 | 0xa0) && packet.length >= 2;
        let part_transpose = if let Some(parts) = stage.keyboard_parts {
            let part = if keyed_message {
                let note = packet.data[1];
                match parts.split_key {
                    Some(split) if note >= split => parts.part_2,
                    _ => parts.part_1,
                }
            } else if parts.split_key.is_some()
                && transform
                    .source_channels
                    .contains(&parts.part_2.midi_channel)
            {
                parts.part_2
            } else {
                parts.part_1
            };
            if !transform.source_channels.is_empty()
                && !transform.source_channels.contains(&part.midi_channel)
            {
                return None;
            }
            part.transpose
        } else {
            let source_channel = (packet.data[0] & 0x0f) + 1;
            if !transform.source_channels.is_empty()
                && !transform.source_channels.contains(&source_channel)
            {
                return None;
            }
            0
        };
        if keyed_message && !(transform.note_low..=transform.note_high).contains(&packet.data[1]) {
            return None;
        }

        if index == 0 {
            packet = play_route.route(event)?.packet;
        }
        if keyed_message {
            let transposed = i16::from(packet.data[1])
                + i16::from(part_transpose)
                + i16::from(transform.transpose);
            if !(0..=127).contains(&transposed) {
                return None;
            }
            packet.data[1] = transposed as u8;
        }
        if status == 0x90 && packet.length >= 3 && packet.data[2] > 0 {
            packet.data[2] = map_midi_velocity(
                packet.data[2],
                transform.velocity_input_low,
                transform.velocity_input_high,
                transform.velocity_output_low,
                transform.velocity_output_high,
            );
        }
        if let Some(channel) = transform.target_channel
            && matches!(status, 0x80..=0xe0)
        {
            packet.data[0] = (packet.data[0] & 0xf0) | (channel - 1);
        }
    }
    Some(plugin_midi_event(packet))
}

fn replay_rack_controller_state(
    controller_states: &MidiControllerStates,
    play_route: &CompiledMidiRoute,
    voice: &mut RackSlotVoice<'_>,
) -> usize {
    if voice
        .midi_stages
        .iter()
        .any(|stage| stage.transform.notes_only)
    {
        return 0;
    }
    let first_channels = voice
        .midi_stages
        .first()
        .map(|stage| stage.transform.source_channels.as_slice())
        .unwrap_or_default();
    let initial_len = voice.events.len();
    let omitted = if first_channels.is_empty() {
        controller_states.replay_routed_into(
            play_route,
            None,
            &mut voice.events,
            MAX_EVENTS_PER_BLOCK,
        )
    } else {
        first_channels
            .iter()
            .map(|channel| {
                controller_states.replay_routed_into(
                    play_route,
                    Some(*channel),
                    &mut voice.events,
                    MAX_EVENTS_PER_BLOCK,
                )
            })
            .sum()
    };

    // The controller-state store replays through the global route first.
    // Compact the appended tail in place while applying the same parent →
    // child channel chain used for live events. Intentional filtering is not
    // counted as a realtime-capacity omission.
    let mut write = initial_len;
    for read in initial_len..voice.events.len() {
        let mut event = voice.events[read];
        let mut accepted = true;
        for (index, stage) in voice.midi_stages.iter().enumerate() {
            let transform = &stage.transform;
            if index > 0 {
                let channel = (event.data[0] & 0x0f) + 1;
                if !transform.source_channels.is_empty()
                    && !transform.source_channels.contains(&channel)
                {
                    accepted = false;
                    break;
                }
            }
            if let Some(channel) = transform.target_channel
                && matches!(event.data[0] & 0xf0, 0x80..=0xe0)
            {
                event.data[0] = (event.data[0] & 0xf0) | (channel - 1);
            }
        }
        if accepted {
            voice.events[write] = event;
            write += 1;
        }
    }
    voice.events.truncate(write);
    omitted
}

#[cfg(test)]
fn route_rack_event(
    event: IngressMidiEvent,
    midi_input_channel: Option<u8>,
    midi_note_low: u8,
    midi_note_high: u8,
    midi_transpose: i8,
    keyboard_parts: Option<RackKeyboardParts>,
    play_route: &CompiledMidiRoute,
) -> Option<MidiEventV1> {
    let transform = rackforge_performance_api::RackMidiTransform {
        source_channels: midi_input_channel.into_iter().collect(),
        target_channel: None,
        note_low: midi_note_low,
        note_high: midi_note_high,
        transpose: midi_transpose,
        notes_only: false,
        velocity_input_low: 0,
        velocity_input_high: 127,
        velocity_output_low: 0,
        velocity_output_high: 127,
    };
    route_rack_event_transformed(event, &transform, keyboard_parts, play_route)
}

fn map_midi_velocity(
    value: u8,
    input_low: u8,
    input_high: u8,
    output_low: u8,
    output_high: u8,
) -> u8 {
    if value <= input_low {
        return output_low;
    }
    if value >= input_high {
        return output_high;
    }
    let input_span = u16::from(input_high - input_low);
    let output_span = u16::from(output_high - output_low);
    let offset = u16::from(value - input_low);
    output_low + ((offset * output_span + input_span / 2) / input_span) as u8
}

fn restore_after_audition(voice: &mut StandaloneVoice<'_>, lease: &AuditionLease) -> Result<()> {
    voice.mirror_control(|instance| instance.reset())?;
    if let Some(previous) = &lease.previous_sound_id {
        voice.mirror_control(|instance| instance.load_preset(previous))?;
    }
    Ok(())
}

fn read_period(
    pcm: &PCM,
    io: &alsa::pcm::IO<'_, i32>,
    input: &mut [i32],
    period_frames: usize,
    channels: usize,
    xruns: &mut XrunMonitor,
) -> Result<()> {
    if channels == 0 || input.len() != period_frames * channels {
        bail!("invalid audio capture buffer layout");
    }
    let mut frame_offset = 0;
    while frame_offset < period_frames {
        match io.readi(&mut input[frame_offset * channels..]) {
            Ok(0) => bail!("audio input returned zero frames"),
            Ok(frames) => frame_offset += frames,
            Err(error) if error.errno() == libc::EPIPE => {
                xruns.record();
                pcm.prepare()?;
                frame_offset = 0;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn map_capture_channels(
    device: &[i32],
    device_channels: usize,
    selected: &[usize],
    gain: f32,
    output: &mut [f32],
) {
    if selected.is_empty() || device_channels == 0 {
        output.fill(0.0);
        return;
    }
    let frames = device.len() / device_channels;
    debug_assert_eq!(output.len(), frames * selected.len());
    for frame in 0..frames {
        for (target, source) in selected.iter().copied().enumerate() {
            let sample = device[frame * device_channels + source] as f32 / i32::MAX as f32;
            output[frame * selected.len() + target] = if sample.is_finite() {
                (sample * gain).clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
    }
}

fn prepare_plugin_capture(
    captured: &[f32],
    capture_channels: usize,
    plugin: &mut [f32],
    plugin_channels: usize,
    frames: usize,
) {
    let destination = &mut plugin[..frames * plugin_channels];
    destination.fill(0.0);
    if capture_channels == 0 || plugin_channels == 0 {
        return;
    }
    for frame in 0..frames {
        for channel in 0..plugin_channels {
            destination[frame * plugin_channels + channel] = if capture_channels == 1 {
                captured[frame * capture_channels]
            } else if plugin_channels == 1 {
                (captured[frame * capture_channels] + captured[frame * capture_channels + 1]) * 0.5
            } else {
                captured
                    .get(frame * capture_channels + channel)
                    .copied()
                    .unwrap_or(0.0)
            };
        }
    }
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
    use crate::live_midi_state::{MidiControllerState, matches_midi_input_channel};
    use rackforge_midi_api::{MidiPacket, MidiSourceId};
    use rackforge_performance_api::{RackKeyboardPart, RackMidiTransform};
    use rackforge_session_api::{
        HostActionBinding, HostActionTarget, HostControlBinding, HostControlTarget,
        MidiButtonBinding, MidiControlChangeBinding,
    };

    fn midi(length: u8, data: [u8; 3]) -> MidiEventV1 {
        MidiEventV1 {
            frame: 0,
            length,
            data,
        }
    }

    #[test]
    fn automatic_audio_workers_scale_with_cpu_capacity() {
        assert_eq!(parallel_render::automatic_audio_worker_capacity(1), 0);
        assert_eq!(parallel_render::automatic_audio_worker_capacity(2), 2);
        assert_eq!(parallel_render::automatic_audio_worker_capacity(4), 3);
        assert_eq!(
            parallel_render::automatic_audio_worker_capacity(64),
            control::MAX_ACTIVE_RACK_SLOTS
        );
    }

    #[test]
    fn the_scheduler_covers_every_control_plane_rack_slot() {
        assert_eq!(
            parallel_render::MAX_RENDER_SLOTS,
            control::MAX_ACTIVE_RACK_SLOTS
        );
    }

    #[test]
    fn physical_capture_mapping_preserves_selection_order_and_gain() {
        let device = [i32::MAX / 4, i32::MAX / 2, -(i32::MAX / 4), -(i32::MAX / 2)];
        let mut mapped = [0.0_f32; 4];
        map_capture_channels(&device, 2, &[1, 0], 2.0, &mut mapped);
        assert!((mapped[0] - 1.0).abs() < 1e-6);
        assert!((mapped[1] - 0.5).abs() < 1e-6);
        assert!((mapped[2] + 1.0).abs() < 1e-6);
        assert!((mapped[3] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn mono_capture_duplicates_to_stereo_and_stereo_averages_to_mono() {
        let mono = [0.25_f32, -0.5];
        let mut stereo = [0.0_f32; 4];
        prepare_plugin_capture(&mono, 1, &mut stereo, 2, 2);
        assert_eq!(stereo, [0.25, 0.25, -0.5, -0.5]);

        let stereo = [0.25_f32, 0.75, -0.5, 0.25];
        let mut mono = [0.0_f32; 2];
        prepare_plugin_capture(&stereo, 2, &mut mono, 1, 2);
        assert_eq!(mono, [0.5, -0.125]);
    }

    #[test]
    fn live_without_an_active_rack_resolves_to_explicit_silence() {
        assert_eq!(
            resolve_render_mode(SurfaceMode::Live, 0),
            AudioRenderMode::Silent
        );
        assert_eq!(
            resolve_render_mode(SurfaceMode::Live, 1),
            AudioRenderMode::Rack
        );
        assert_eq!(
            resolve_render_mode(SurfaceMode::Play, 0),
            AudioRenderMode::Plugin
        );
        assert_eq!(
            resolve_render_mode(SurfaceMode::Idle, 3),
            AudioRenderMode::Silent
        );
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
            states.replay_source_into(MidiSourceKey::new(0), &mut first, MAX_EVENTS_PER_BLOCK,),
            Some(0)
        );
        assert_eq!(first[0].data, [0xb0, 1, 20]);

        let mut second = Vec::new();
        assert_eq!(
            states.replay_source_into(MidiSourceKey::new(1), &mut second, MAX_EVENTS_PER_BLOCK,),
            Some(0)
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
    fn rack_midi_connection_filters_multiple_channels_and_transforms_output() {
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
        let transform = RackMidiTransform {
            source_channels: vec![2, 4, 10],
            target_channel: Some(9),
            note_low: 36,
            note_high: 84,
            transpose: 12,
            notes_only: true,
            velocity_input_low: 20,
            velocity_input_high: 100,
            velocity_output_low: 40,
            velocity_output_high: 110,
        };

        let routed = route_rack_event_transformed(event(&[0x93, 48, 60]), &transform, None, &route)
            .expect("selected source channel should reach the connection");
        assert_eq!(routed.data, [0x98, 60, 75]);
        assert!(
            route_rack_event_transformed(event(&[0x94, 48, 60]), &transform, None, &route,)
                .is_none()
        );
        assert!(
            route_rack_event_transformed(event(&[0xb3, 1, 64]), &transform, None, &route,)
                .is_none()
        );
    }

    #[test]
    fn nested_rack_midi_connections_run_in_parent_to_child_order() {
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
        let stages = [
            RackMidiStageRuntimeSpec {
                transform: RackMidiTransform {
                    source_channels: vec![4],
                    target_channel: Some(7),
                    note_low: 36,
                    note_high: 72,
                    transpose: 12,
                    ..RackMidiTransform::default()
                },
                keyboard_parts: None,
            },
            RackMidiStageRuntimeSpec {
                transform: RackMidiTransform {
                    source_channels: vec![7],
                    target_channel: Some(9),
                    note_low: 60,
                    note_high: 84,
                    transpose: -12,
                    ..RackMidiTransform::default()
                },
                keyboard_parts: None,
            },
        ];

        let routed = route_rack_event_through_stages(event(&[0x93, 48, 100]), &stages, &route)
            .expect("the child Rack should receive the parent-mapped event");
        assert_eq!(routed.data, [0x98, 48, 100]);
        assert!(
            route_rack_event_through_stages(event(&[0x92, 48, 100]), &stages, &route).is_none()
        );

        let mut rejecting_child = stages.clone();
        rejecting_child[1].transform.source_channels = vec![8];
        assert!(
            route_rack_event_through_stages(event(&[0x93, 48, 100]), &rejecting_child, &route,)
                .is_none()
        );
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
