//! A LIVE Rack's Slots as an engine renders them.
//!
//! This was the appliance's alone: it lived in `live.rs`, behind the Linux
//! gate, so every other host could load a Rack and play only its first
//! instrument. Nothing here touches a device. It is the Slots -- their
//! plugin instances, the MIDI each one hears through its stages, the audio
//! cables between them, their level and pan -- and the mix they make, so the
//! appliance's ALSA loop and Android's render worker play one Rack the same
//! way.
//!
//! The appliance's loop drives the pieces directly, because it also stages
//! hardware capture and telemetry between them. A host without that loop
//! uses [`RackEngine`], which is the same pieces in the same order.

use crate::capture_route::CaptureRoute;
use crate::midi2::Midi2Event;
use crate::parallel_render::{
    self, ParallelUnits, RenderPool, RenderTelemetry, ScheduledSlot, UnitJob,
    process_slots_sequential,
};
use crate::rack_graph::{CompiledAudioSource, compile_instrument_definition};
use crate::realtime_budget::{self, BudgetGovernor, BudgetReason};
use crate::{CompiledParameterLink, LoadedPlugin, PluginInstance, PluginStateStore};
use anyhow::{Context, Result, bail};
use rackforge_midi_api::{CompiledMidiRoute, IngressMidiEvent, ParameterLinkPassThrough};
use rackforge_performance_api::{
    PerformanceLibrary, RackDefinition, RackKeyboardParts, RackMidiTransform,
};
use rackforge_plugin_api::abi::ParameterEventV1;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

/// The most Slots one Rack may compile to. A Slot's upstream Slots are a
/// bit mask, and the scheduler's slot table is sized for this.
pub const MAX_ACTIVE_RACK_SLOTS: usize = 8;
/// The most MIDI or parameter events one Slot takes in one block.
pub const MAX_EVENTS_PER_BLOCK: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RackSlotStateLoad {
    Default,
    Opaque(Vec<u8>),
    LegacyPreset(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RackMidiStageRuntimeSpec {
    pub transform: RackMidiTransform,
    pub keyboard_parts: Option<RackKeyboardParts>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RackSlotRuntimeSpec {
    pub slot_id: String,
    pub plugin_id: String,
    pub state: RackSlotStateLoad,
    pub midi_stages: Vec<RackMidiStageRuntimeSpec>,
    pub audio_sources: Vec<CompiledAudioSource>,
    pub sends_to_main: bool,
    pub level_per_mille: u16,
    pub pan_per_mille: i16,
}

/// A saved Rack (or a Song Part's graph) as the Slots an engine builds: the
/// graph compiled, each Slot's sound read from the store.
pub fn rack_runtime_specs(
    library: &PerformanceLibrary,
    rack: &RackDefinition,
    state_store: &PluginStateStore,
) -> Result<Vec<RackSlotRuntimeSpec>> {
    let compiled_slots = compile_instrument_definition(library, rack)?;
    if compiled_slots.len() > MAX_ACTIVE_RACK_SLOTS {
        bail!(
            "Rack {} compiles to {} Slots; this engine supports at most {MAX_ACTIVE_RACK_SLOTS}",
            rack.id,
            compiled_slots.len(),
        );
    }
    let mut specs = Vec::with_capacity(compiled_slots.len());
    for compiled in compiled_slots {
        let slot = &compiled.slot;
        let state = if let Some(reference) = &slot.state {
            RackSlotStateLoad::Opaque(
                state_store
                    .read(reference)
                    .with_context(|| format!("reading the sound of Rack Slot {}", slot.id))?,
            )
        } else if let Some(program_id) = &slot.legacy_program_id {
            RackSlotStateLoad::LegacyPreset(program_id.clone())
        } else {
            RackSlotStateLoad::Default
        };
        specs.push(RackSlotRuntimeSpec {
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
    Ok(specs)
}

/// One audio source of a Slot, resolved to indices once per activation so
/// the per-block gather performs no string comparisons.
#[derive(Clone, Copy)]
pub(crate) enum ResolvedRackSource {
    /// This cable's share of the hardware capture staged for the block.
    Capture(CaptureRoute),
    /// The finished output of an earlier Slot in the compiled order.
    Slot(usize),
}

pub(crate) struct RackSlotVoice<'plugin> {
    pub(crate) slot_id: String,
    pub(crate) plugin: &'plugin LoadedPlugin,
    pub(crate) instance: PluginInstance<'plugin>,
    /// Host-owned unit instances for `parallel_render_v1` plugins; `None`
    /// keeps the Slot on the classic indivisible render path.
    pub(crate) parallel: Option<ParallelUnits<'plugin>>,
    pub(crate) midi_stages: Vec<RackMidiStageRuntimeSpec>,
    pub(crate) audio_sources: Vec<CompiledAudioSource>,
    /// `audio_sources` resolved against the compiled Slot order.
    pub(crate) resolved_sources: Vec<ResolvedRackSource>,
    /// Bitmask of the earlier Slots feeding this one; the scheduler holds
    /// this Slot until every one of them completed its block.
    pub(crate) deps_mask: u32,
    /// Hardware capture staged for the current block; rewritten by the
    /// audio loop before every Rack render.
    pub(crate) capture_ptr: *const f32,
    pub(crate) capture_len: usize,
    pub(crate) capture_channels: usize,
    /// The physical input (one-based) of each captured channel, in order,
    /// staged with the capture: a cable names inputs, not positions.
    pub(crate) capture_inputs_ptr: *const u32,
    pub(crate) capture_inputs_len: usize,
    pub(crate) sends_to_main: bool,
    pub(crate) input_channels: usize,
    pub(crate) level: f32,
    pub(crate) pan: f32,
    pub(crate) input: Vec<f32>,
    pub(crate) output: Vec<f32>,
    pub(crate) events: Vec<Midi2Event>,
    /// The events as the parallel scheduler takes them, rebuilt each block.
    pub(crate) parameter_events: Vec<ParameterEventV1>,
    pub(crate) process_faulted: bool,
    /// When this block's `begin` started, so the budget loop can be closed
    /// at `end` with the wall time the WHOLE block took.
    ///
    /// The classic path measures one call and tells the governor about it. A
    /// parallel block is three calls with four workers in between, and
    /// nothing was telling the governor anything at all: both call sites
    /// were in `process_wide`, so an instrument split across cores never
    /// learned it was late and never gave ground. It ran La Campanella 2280
    /// blocks past the deadline without one cut.
    pub(crate) block_started: Option<Instant>,
    pub(crate) budget: SlotBudget,
}

/// Resolves every Slot's cable sources to indices and dependency masks.
/// Runs at activation, never per block. A source that does not name an
/// earlier Slot is dropped, exactly as the previous sequential graph walk
/// ignored it: the compiled order is topological, so a forward reference
/// would be a compiler bug rather than a playable graph.
pub(crate) fn resolve_rack_voice_graph(voices: &mut [RackSlotVoice<'_>]) {
    for index in 0..voices.len() {
        let (earlier, rest) = voices.split_at_mut(index);
        let voice = &mut rest[0];
        voice.resolved_sources.clear();
        voice.deps_mask = 0;
        for source in &voice.audio_sources {
            match source {
                CompiledAudioSource::HardwareInput { route, .. } => {
                    voice
                        .resolved_sources
                        .push(ResolvedRackSource::Capture(CaptureRoute::new(route)));
                }
                CompiledAudioSource::Slot { runtime_slot_id } => {
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
                            "LIVE_RACK_SOURCE_IGNORED slot={} source={runtime_slot_id} \
                             reason=not-an-earlier-slot",
                            voice.slot_id
                        );
                    }
                }
            }
        }
    }
}

/// One Slot's side of `realtime_budget`: the governor, the clock it measures
/// against, and whatever it has decided but not yet had a chance to report.
///
/// Lives on the voice because that is what the worker thread owns for the
/// duration of a block. Nothing here formats a string or allocates: the
/// decision is left in `pending` and the audio loop moves it into telemetry
/// once the block is finished.
pub(crate) struct SlotBudget {
    pub(crate) governor: BudgetGovernor,
    pub(crate) epoch: Instant,
    /// When a MIDI event last reached this Slot. Quality is only ever given
    /// back after `SILENT_BEFORE_RAISE` of this standing still.
    pub(crate) pending: Option<(u64, BudgetReason, f64)>,
    /// `None` until the plugin has been asked whether it takes a budget at
    /// all. Most plugins do not, and those are never asked twice.
    pub(crate) participates: Option<bool>,
    /// What this machine settled on for this plugin before, per period,
    /// copied out of the store on the control thread. The first block whose
    /// period is known seeds the governor from it -- a scan of a few pairs,
    /// no lock, on the render thread.
    pub(crate) remembered: Vec<(u64, u64)>,
    pub(crate) seeded: bool,
    /// Whether this session's settled budget has been handed to the store.
    pub(crate) stored: bool,
}

impl SlotBudget {
    /// Built on the control thread, where reading the store is allowed.
    pub(crate) fn for_plugin(plugin: &LoadedPlugin) -> Self {
        Self {
            governor: BudgetGovernor::default(),
            epoch: Instant::now(),
            pending: None,
            participates: None,
            remembered: realtime_budget::remembered(&plugin.descriptor().id),
            seeded: false,
            stored: false,
        }
    }
}

/// Closes the budget loop around one finished block.
///
/// Called on the worker thread that just rendered the Slot, with the wall time
/// that render took. The plugin is only ever entered here between blocks, and
/// at most once every couple of seconds, which is the whole reason a plugin is
/// allowed to rebuild coefficients when it is told.
pub(crate) fn observe_budget(
    budget: &mut SlotBudget,
    instance: &mut PluginInstance<'_>,
    render_ns: u64,
) {
    if budget.participates == Some(false) {
        return;
    }
    let now_instant = Instant::now();
    if !budget.seeded {
        // Once, on the first block whose period is known: what last session
        // settled on for this plugin at this period, if anything.
        budget.seeded = true;
        let deadline = budget.governor.deadline_ns();
        if let Some((_, fuel)) = budget.remembered.iter().find(|(d, _)| *d == deadline) {
            budget.governor.seed(*fuel);
        }
    }
    if budget.participates.is_none() {
        budget.participates = Some(instance.accepts_realtime_budget());
        if budget.participates == Some(false) {
            return;
        }
    }
    let Some(fuel) = instance.last_realtime_fuel_consumed() else {
        // Not metered by the sandbox, so there is no honest budget to state.
        budget.participates = Some(false);
        return;
    };
    budget.governor.observe(render_ns, fuel);
    let now = now_instant.saturating_duration_since(budget.epoch);
    // Never raised within a session. A raise rebuilds banks, and even faded
    // through silence it is a change the player hears coming and going:
    // measured on the appliance, quality came back between pieces and the
    // next dense passage cut it again, every piece. In a session quality only
    // goes down, rarely, when the machine proves it must; it comes back the
    // next time the engine starts, from what the store remembers.
    let Some((granted, reason)) = budget.governor.poll(now, false) else {
        return;
    };
    match instance.set_realtime_budget(granted) {
        Ok(true) => {
            let rate = budget.governor.nanoseconds_per_fuel().unwrap_or(0.0);
            budget.pending = Some((granted, reason, rate));
        }
        Ok(false) => budget.participates = Some(false),
        Err(error) => {
            // A trap in a control call is the plugin's fault, not the block's.
            // Stop asking rather than risk it every couple of seconds.
            budget.participates = Some(false);
            eprintln!("PLUGIN_BUDGET_REFUSED action=stop-asking error={error:#}");
        }
    }
}

/// Moves a Slot's budget decision into telemetry, once the block it was made
/// in is over.
///
/// Two relaxed stores. The publisher thread turns them into a log line a
/// second later, because formatting one on the audio thread would allocate.
pub(crate) fn report_budget(
    budget: &mut SlotBudget,
    slot: usize,
    telemetry: &Arc<RenderTelemetry>,
) {
    let deadline_ns = budget.governor.deadline_ns();
    let window = budget.governor.last_window();
    let counts = budget.governor.last_counts();
    if let Some((fuel, reason, rate)) = budget.pending.take() {
        telemetry.record_budget(
            slot,
            fuel,
            reason.as_str(),
            rate,
            deadline_ns,
            window,
            counts,
        );
        return;
    }
    if budget.stored {
        return;
    }
    // Once the budget has stopped moving it is worth remembering; the
    // publisher thread writes it down under the plugin's name.
    let now = Instant::now().saturating_duration_since(budget.epoch);
    if let Some(settled) = budget.governor.settled(now) {
        budget.stored = true;
        let rate = budget.governor.nanoseconds_per_fuel().unwrap_or(0.0);
        telemetry.record_budget(
            slot,
            settled,
            BudgetReason::Settled.as_str(),
            rate,
            deadline_ns,
            window,
            counts,
        );
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
                ResolvedRackSource::Capture(route) => {
                    if voice.capture_len == 0 || voice.capture_channels == 0 {
                        continue;
                    }
                    // SAFETY: both staged by the audio loop for this block,
                    // from buffers it owns for the whole loop, and only read
                    // during the block.
                    let capture =
                        unsafe { std::slice::from_raw_parts(voice.capture_ptr, voice.capture_len) };
                    let inputs = unsafe {
                        std::slice::from_raw_parts(
                            voice.capture_inputs_ptr,
                            voice.capture_inputs_len,
                        )
                    };
                    route.mix_into(
                        capture,
                        voice.capture_channels,
                        inputs,
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
        self.block_started = Some(Instant::now());
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
        let finished = parallel
            .finish(
                &mut self.instance,
                &mut self.output,
                frames,
                channels,
                completed,
            )
            .is_ok();
        // The budget loop, closed where the block actually ends. The fuel the
        // coordinator reports now is the whole block's: the orchestrator adds
        // what planning cost and what the four workers spent in their own
        // instances.
        if let (true, Some(started)) = (finished, self.block_started.take()) {
            observe_budget(
                &mut self.budget,
                &mut self.instance,
                started.elapsed().as_nanos() as u64,
            );
        }
        finished
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
pub(crate) fn create_rack_slot_parallel_units<'plugin>(
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

pub(crate) fn process_rack_voice(voice: &mut RackSlotVoice<'_>, period_frames: u32, channels: u32) {
    voice.output.fill(0.0);
    if voice.process_faulted {
        return;
    }
    let started = Instant::now();
    let process_result = voice.instance.process_wide(
        &voice.input,
        &mut voice.output,
        period_frames,
        voice.input_channels as u32,
        channels,
        &voice.events,
        &voice.parameter_events,
    );
    let render_ns = started.elapsed().as_nanos() as u64;
    if process_result.is_ok() {
        observe_budget(&mut voice.budget, &mut voice.instance, render_ns);
    }
    if let Err(error) = process_result {
        voice.output.fill(0.0);
        voice.process_faulted = true;
        eprintln!(
            "PLUGIN_PROCESS_QUARANTINED context=rack-slot:{} action=silence error={error}",
            voice.slot_id
        );
    }
}

pub(crate) fn mix_slot_into_plugin(
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

/// One Slot's output into the Rack's mix, at its level and pan.
pub(crate) fn mix_rack_slot(
    mix: &mut [f32],
    source: &[f32],
    channels: usize,
    level: f32,
    pan: f32,
) {
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

pub(crate) fn plugin_audio_channels(plugin: &LoadedPlugin) -> Result<(usize, usize)> {
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

pub(crate) fn create_rack_voices<'plugin>(
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
            block_started: None,
            midi_stages: spec.midi_stages.clone(),
            audio_sources: spec.audio_sources.clone(),
            resolved_sources: Vec::new(),
            deps_mask: 0,
            capture_ptr: std::ptr::null(),
            capture_len: 0,
            capture_channels: 0,
            capture_inputs_ptr: std::ptr::null(),
            capture_inputs_len: 0,
            sends_to_main: spec.sends_to_main,
            input_channels,
            level: f32::from(spec.level_per_mille) / 1_000.0,
            pan: f32::from(spec.pan_per_mille) / 1_000.0,
            input: vec![0.0; period_frames as usize * input_channels],
            output: vec![0.0; period_frames as usize * channels as usize],
            events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            parameter_events: Vec::with_capacity(MAX_EVENTS_PER_BLOCK),
            process_faulted: false,
            budget: SlotBudget::for_plugin(plugin),
        });
    }
    resolve_rack_voice_graph(&mut voices);
    Ok(voices)
}

/// One incoming event as a Slot hears it, through each of its stages from
/// the outermost graph inwards: its channels, its note range, its keyboard
/// split and transposition, its velocity curve, its output channel.
///
/// The appliance first routes the event through its play route (which
/// source, which channel model); a host without routes passes `None` and
/// the event enters the stages as it arrived.
pub(crate) fn route_through_stages(
    event: IngressMidiEvent,
    stages: &[RackMidiStageRuntimeSpec],
    play_route: Option<&CompiledMidiRoute>,
) -> Option<Midi2Event> {
    let routed_packet = |event: IngressMidiEvent| match play_route {
        Some(route) => route.route(event).map(|routed| routed.packet),
        None => Some(event.packet),
    };
    if stages.is_empty() {
        return routed_packet(event).map(|packet| Midi2Event::from_packet(&packet));
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
            packet = routed_packet(event)?;
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
            // A velocity with more than seven bits rides the same curve at
            // its own width -- the byte endpoints scaled up by the
            // specification's rule, so an endpoint means the same loudness
            // on both scales -- and its byte projection follows it.
            if let Some(value) = packet.wide {
                let mapped = map_wide_velocity(
                    value & 0xffff,
                    transform.velocity_input_low,
                    transform.velocity_input_high,
                    transform.velocity_output_low,
                    transform.velocity_output_high,
                );
                packet.wide = Some(mapped);
                packet.data[2] = ((mapped >> 9) as u8).max(1);
            }
        }
        if let Some(channel) = transform.target_channel
            && matches!(status, 0x80..=0xe0)
        {
            packet.data[0] = (packet.data[0] & 0xf0) | (channel - 1);
        }
    }
    Some(Midi2Event::from_packet(&packet))
}

pub(crate) fn map_midi_velocity(
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

/// `map_midi_velocity` at sixteen bits: the same curve, its endpoints
/// lifted by the specification's scaling so that `0..=127` is the identity
/// on the whole 16-bit range and a byte-valued endpoint means the same
/// loudness it means on the byte scale.
pub(crate) fn map_wide_velocity(
    value: u32,
    input_low: u8,
    input_high: u8,
    output_low: u8,
    output_high: u8,
) -> u32 {
    let lift = |byte: u8| crate::midi2::scale_up(u32::from(byte), 7, 16);
    let (input_low, input_high) = (lift(input_low), lift(input_high));
    let (output_low, output_high) = (lift(output_low), lift(output_high));
    if value <= input_low {
        return output_low;
    }
    if value >= input_high {
        return output_high;
    }
    let input_span = u64::from(input_high - input_low);
    let output_span = u64::from(output_high - output_low);
    let offset = u64::from(value - input_low);
    output_low + ((offset * output_span + input_span / 2) / input_span) as u32
}

/// A whole Rack for a host that renders in blocks of its own choosing:
/// every Slot built and activated, MIDI routed to each through its stages,
/// the Slots rendered on the shared pool in their graph order, and their
/// outputs mixed at their level and pan.
///
/// It owns nothing of the device. The host hands it a block's MIDI and an
/// interleaved stereo buffer; everything between is the appliance's own
/// Rack code, so a Rack sounds the same on every host that runs it.
pub struct RackEngine<'plugin> {
    voices: Vec<RackSlotVoice<'plugin>>,
    channels: usize,
    max_frames: usize,
    /// Links to this Rack's Slots, compiled against their plugins.
    parameter_links: Vec<CompiledParameterLink>,
    dropped_events: u64,
}

impl<'plugin> RackEngine<'plugin> {
    /// Builds and activates every Slot. `max_frames` is the largest block
    /// the host will ask for; smaller blocks are rendered in place.
    pub fn build(
        plugins: &BTreeMap<String, &'plugin LoadedPlugin>,
        specs: &[RackSlotRuntimeSpec],
        sample_rate_hz: u32,
        max_frames: u32,
        channels: u32,
    ) -> Result<Self> {
        if specs.is_empty() {
            bail!("the Rack has no Slot that plays");
        }
        if specs.len() > MAX_ACTIVE_RACK_SLOTS {
            bail!(
                "the Rack compiles to {} Slots; this engine supports at most {MAX_ACTIVE_RACK_SLOTS}",
                specs.len()
            );
        }
        if channels != 2 {
            bail!("a Rack renders stereo; {channels} channels were asked for");
        }
        let voices = create_rack_voices(plugins, specs, sample_rate_hz, max_frames, channels)?;
        Ok(Self {
            voices,
            channels: channels as usize,
            max_frames: max_frames as usize,
            parameter_links: Vec::new(),
            dropped_events: 0,
        })
    }

    pub fn slot_count(&self) -> usize {
        self.voices.len()
    }

    /// Each Slot's runtime id (its Rack path) and plugin id, in render order.
    pub fn slots(&self) -> impl Iterator<Item = (&str, &str)> {
        self.voices
            .iter()
            .map(|voice| (voice.slot_id.as_str(), voice.plugin.manifest().id.as_str()))
    }

    /// The plugin a parameter link's target names, when it is one of this
    /// Rack's Slots.
    pub fn link_target_plugin(&self, target: &str) -> Option<&'plugin LoadedPlugin> {
        self.voices
            .iter()
            .find(|voice| crate::rack_graph::voice_matches_link_target(&voice.slot_id, target))
            .map(|voice| voice.plugin)
    }

    /// The links to this Rack's Slots, compiled by the host against
    /// [`Self::link_target_plugin`].
    pub fn set_parameter_links(&mut self, links: Vec<CompiledParameterLink>) {
        self.parameter_links = links;
    }

    /// Events a Slot could not take because its block was full, since the
    /// last time this was asked.
    pub fn take_dropped_events(&mut self) -> u64 {
        std::mem::take(&mut self.dropped_events)
    }

    /// One incoming event, to every Slot that hears it. A control linked to
    /// a Slot's parameter moves the parameter, and is kept from the Slots
    /// when the link consumes it.
    pub fn route(&mut self, event: IngressMidiEvent, play_route: Option<&CompiledMidiRoute>) {
        for voice in &mut self.voices {
            let mut consume = false;
            for link in self.parameter_links.iter_mut().filter(|link| {
                crate::rack_graph::voice_matches_link_target(&voice.slot_id, &link.link.instance_id)
            }) {
                let Some(mapped) = link.apply(event, |_| None) else {
                    continue;
                };
                consume |= mapped.pass_through == ParameterLinkPassThrough::Consume;
                if voice.parameter_events.len() < MAX_EVENTS_PER_BLOCK {
                    voice.parameter_events.push(mapped.event);
                } else {
                    self.dropped_events += 1;
                }
            }
            if consume {
                continue;
            }
            if let Some(routed) = route_through_stages(event, &voice.midi_stages, play_route) {
                if voice.events.len() < MAX_EVENTS_PER_BLOCK {
                    voice.events.push(routed);
                } else {
                    self.dropped_events += 1;
                }
            }
        }
    }

    /// Renders one block and writes the Rack's mix into `output`, an
    /// interleaved stereo buffer of `frames` frames. The block's MIDI and
    /// parameter events are spent.
    pub fn render(
        &mut self,
        pool: &mut RenderPool,
        telemetry: &Arc<RenderTelemetry>,
        frames: u32,
        deadline_ns: u64,
        output: &mut [f32],
    ) -> Result<()> {
        let block = frames as usize;
        if block == 0 || block > self.max_frames {
            bail!(
                "a Rack block of {frames} frames is outside 1..={}",
                self.max_frames
            );
        }
        if output.len() != block * self.channels {
            bail!("the Rack's output buffer does not hold {frames} stereo frames");
        }
        let slot_count = self.voices.len();
        for voice in &mut self.voices {
            // Sized to this block within what was reserved at build, so a
            // host's short block never allocates on the render thread.
            voice.input.resize(block * voice.input_channels, 0.0);
            voice.output.resize(block * self.channels, 0.0);
            voice.budget.governor.configure(deadline_ns, slot_count);
        }
        let started = Instant::now();
        if !pool.process(&mut self.voices, frames, self.channels as u32, deadline_ns) {
            process_slots_sequential(&mut self.voices, frames, self.channels as u32, telemetry);
            telemetry.record_block(started.elapsed().as_nanos() as u64, deadline_ns, None);
        }
        output.fill(0.0);
        for (slot, voice) in self.voices.iter_mut().enumerate() {
            report_budget(&mut voice.budget, slot, telemetry);
            if !voice.process_faulted && voice.sends_to_main {
                mix_rack_slot(output, &voice.output, self.channels, voice.level, voice.pan);
            }
            voice.events.clear();
            voice.parameter_events.clear();
        }
        Ok(())
    }

    /// Every Slot's notes let go and its voices silenced: a panic, or the
    /// Rack about to be left.
    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.events.clear();
            voice.parameter_events.clear();
            if let Err(error) = voice.instance.reset() {
                eprintln!(
                    "LIVE_RACK_RESET_FAILED slot={} error={error:#}",
                    voice.slot_id
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_midi_api::{MidiPacket, MidiSourceKey};
    use rackforge_performance_api::RackKeyboardPart;

    fn event(bytes: &[u8]) -> IngressMidiEvent {
        let mut data = [0; 3];
        data[..bytes.len()].copy_from_slice(bytes);
        IngressMidiEvent {
            source: MidiSourceKey::new(0),
            packet: MidiPacket {
                frame: 0,
                length: bytes.len() as u8,
                data,
                wide: None,
            },
        }
    }

    fn stage(transform: RackMidiTransform) -> RackMidiStageRuntimeSpec {
        RackMidiStageRuntimeSpec {
            transform,
            keyboard_parts: None,
        }
    }

    fn note_of(routed: Option<Midi2Event>) -> Option<u8> {
        routed.map(|event| event.to_midi1().data[1])
    }

    #[test]
    fn with_no_play_route_an_event_enters_the_stages_as_it_arrived() {
        let routed = route_through_stages(event(&[0x90, 60, 100]), &[], None);
        assert_eq!(note_of(routed), Some(60));
    }

    #[test]
    fn a_split_sends_each_half_of_the_keyboard_to_its_own_slot() {
        let parts = RackKeyboardParts {
            split_key: Some(60),
            part_1: RackKeyboardPart {
                midi_channel: 1,
                transpose: 0,
            },
            part_2: RackKeyboardPart {
                midi_channel: 2,
                transpose: 0,
            },
        };
        let lower = RackMidiStageRuntimeSpec {
            transform: RackMidiTransform {
                source_channels: vec![1],
                ..RackMidiTransform::default()
            },
            keyboard_parts: Some(parts),
        };
        let upper = RackMidiStageRuntimeSpec {
            transform: RackMidiTransform {
                source_channels: vec![2],
                ..RackMidiTransform::default()
            },
            keyboard_parts: Some(parts),
        };
        let low_key = event(&[0x90, 48, 100]);
        let high_key = event(&[0x90, 72, 100]);
        assert_eq!(
            note_of(route_through_stages(low_key, &[lower.clone()], None)),
            Some(48)
        );
        assert_eq!(
            note_of(route_through_stages(high_key, &[lower], None)),
            None
        );
        assert_eq!(
            note_of(route_through_stages(low_key, &[upper.clone()], None)),
            None
        );
        assert_eq!(
            note_of(route_through_stages(high_key, &[upper], None)),
            Some(72)
        );
    }

    #[test]
    fn a_stage_transposes_and_keeps_to_its_range() {
        let up_an_octave = stage(RackMidiTransform {
            transpose: 12,
            note_low: 36,
            note_high: 84,
            ..RackMidiTransform::default()
        });
        assert_eq!(
            note_of(route_through_stages(
                event(&[0x90, 60, 100]),
                &[up_an_octave.clone()],
                None
            )),
            Some(72)
        );
        assert_eq!(
            note_of(route_through_stages(
                event(&[0x90, 90, 100]),
                &[up_an_octave],
                None
            )),
            None
        );
    }

    #[test]
    fn the_mix_places_a_slot_at_its_level_and_pan() {
        let mut mix = vec![0.0; 4];
        mix_rack_slot(&mut mix, &[1.0, 1.0, 1.0, 1.0], 2, 0.5, 1.0);
        assert_eq!(mix, vec![0.0, 0.5, 0.0, 0.5]);
        mix_rack_slot(&mut mix, &[1.0, 1.0, 1.0, 1.0], 2, 1.0, 0.0);
        assert_eq!(mix, vec![1.0, 1.5, 1.0, 1.5]);
    }
}
