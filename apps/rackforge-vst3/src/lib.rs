#![allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]

mod diagnostic;
mod engine;
mod view;
#[cfg(windows)]
mod web_host;
#[cfg(windows)]
mod webview_env;

use engine::{RackForgeEngine, VstPluginModel};
use rackforge_core::midi2::{Midi2Event, Midi2Message};
use rackforge_plugin_api::{
    ParameterDescriptor, ParameterKind, ParameterTaper,
    abi::{MidiEventV1, ParameterEventV1},
};
use std::{
    collections::BTreeMap,
    ffi::{CStr, CString, c_char, c_void},
    ptr, slice,
    str::FromStr,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicU64, Ordering},
    },
};
use vst3::{Class, ComPtr, ComRef, ComWrapper, Steinberg::Vst::*, Steinberg::*, uid};

const PLUGIN_NAME: &str = "RackForge";
const MASTER_LEVEL: ParamID = 0;
const PLUGIN_SELECTION: ParamID = 1;
const PLUGIN_PARAMETER_BASE: ParamID = 0x1_0000;
const MIDI_PARAMETER_BASE: ParamID = 0x1000;
const MIDI_CONTROLLERS_PER_CHANNEL: u32 = 130;
const MIDI_CHANNELS: u32 = 16;
const MIDI_PARAMETER_COUNT: u32 = MIDI_CONTROLLERS_PER_CHANNEL * MIDI_CHANNELS;
const STATE_MAGIC: &[u8; 8] = b"RFVST3\0\0";
const STATE_VERSION: u32 = 2;

// The generated VST3 bindings expose SDK enum constants as signed integers on
// Windows and unsigned integers on Unix. Normalize that ABI difference at the
// assignment boundary instead of spreading target-specific casts throughout
// the factory implementation.
#[cfg(windows)]
const fn vst3_enum_i32(value: i32) -> i32 {
    value
}

#[cfg(not(windows))]
const fn vst3_enum_i32(value: u32) -> i32 {
    value as i32
}

#[cfg(windows)]
const fn vst3_enum_u32(value: i32) -> u32 {
    value as u32
}

#[cfg(not(windows))]
const fn vst3_enum_u32(value: u32) -> u32 {
    value
}

struct ProcessorInner {
    engines: Vec<RackForgeEngine>,
    active_engine: usize,
    sample_rate: f64,
    maximum_frames: usize,
    pending_state: Vec<u8>,
}

impl ProcessorInner {
    /// Drop the engines, keeping what they hold.
    ///
    /// Reactivation restores `pending_state`, and `pending_state` was only ever
    /// written by `setState` -- which a host calls when it loads a project, not
    /// when a fader moves. Everything the player touched after that lived
    /// solely inside the engine, so clearing the engines threw it away and the
    /// next activation brought back the values the project was opened with.
    ///
    /// Both places that tear the engines down are on the path a host walks to
    /// render offline: it deactivates, calls `setupProcessing` for the export's
    /// rate and block size, and activates again. The bounce itself still came
    /// out right, because the host knows the parameter values -- `performEdit`
    /// told it -- and replays them into `process`; what did not survive was the
    /// instrument's own state, so afterwards the panel drew the positions the
    /// player had left while the engine underneath held the project's old ones.
    /// Reported from FL Studio as faders whose value had reset without moving.
    fn stash_and_clear(&mut self) {
        let active = self.active_engine;
        if let Some(engine) = self.engines.get_mut(active)
            && let Ok(state) = engine.save_state()
        {
            self.pending_state = state;
        }
        self.engines.clear();
    }
}

struct RackForgeProcessor {
    inner: Mutex<ProcessorInner>,
    level: AtomicU64,
    models: Vec<Arc<VstPluginModel>>,
}

impl Class for RackForgeProcessor {
    type Interfaces = (IComponent, IAudioProcessor, IProcessContextRequirements);
}

impl RackForgeProcessor {
    const CID: TUID = uid(0x6D4E5B5A, 0x41014BE2, 0x9D8B31E2, 0xF63CA701);

    fn new() -> Self {
        let models = engine::load_bundled_plugin_models()
            .map(|models| models.into_iter().map(Arc::new).collect())
            .inspect_err(|error| {
                diagnostic::write(format!("processor plugin models unavailable: {error:#}"));
            })
            .unwrap_or_default();
        Self {
            inner: Mutex::new(ProcessorInner {
                engines: Vec::new(),
                active_engine: 0,
                sample_rate: 48_000.0,
                maximum_frames: 2048,
                pending_state: Vec::new(),
            }),
            level: AtomicU64::new(1.0_f64.to_bits()),
            models,
        }
    }

    fn level(&self) -> f64 {
        f64::from_bits(self.level.load(Ordering::Relaxed)).clamp(0.0, 1.0)
    }
}

impl IPluginBaseTrait for RackForgeProcessor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }

    unsafe fn terminate(&self) -> tresult {
        if let Ok(mut inner) = self.inner.lock() {
            inner.engines.clear();
        }
        kResultOk
    }
}

impl IComponentTrait for RackForgeProcessor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        diagnostic::write("processor.getControllerClassId");
        if class_id.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            *class_id = RackForgeController::CID;
        }
        kResultOk
    }

    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult {
        kResultOk
    }

    unsafe fn getBusCount(&self, media_type: MediaType, direction: BusDirection) -> i32 {
        match (media_type as MediaTypes, direction as BusDirections) {
            (MediaTypes_::kAudio, BusDirections_::kOutput) => 1,
            (MediaTypes_::kEvent, BusDirections_::kInput) => 1,
            _ => 0,
        }
    }

    unsafe fn getBusInfo(
        &self,
        media_type: MediaType,
        direction: BusDirection,
        index: i32,
        info: *mut BusInfo,
    ) -> tresult {
        if info.is_null() || index != 0 {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        match (media_type as MediaTypes, direction as BusDirections) {
            (MediaTypes_::kAudio, BusDirections_::kOutput) => {
                info.mediaType = MediaTypes_::kAudio as MediaType;
                info.direction = BusDirections_::kOutput as BusDirection;
                info.channelCount = 2;
                copy_wstring("Stereo Output", &mut info.name);
            }
            (MediaTypes_::kEvent, BusDirections_::kInput) => {
                info.mediaType = MediaTypes_::kEvent as MediaType;
                info.direction = BusDirections_::kInput as BusDirection;
                info.channelCount = 16;
                copy_wstring("MIDI Input", &mut info.name);
            }
            _ => return kInvalidArgument,
        }
        info.busType = BusTypes_::kMain as BusType;
        info.flags = vst3_enum_u32(BusInfo_::BusFlags_::kDefaultActive);
        kResultOk
    }

    unsafe fn getRoutingInfo(
        &self,
        _input: *mut RoutingInfo,
        _output: *mut RoutingInfo,
    ) -> tresult {
        kNotImplemented
    }

    unsafe fn activateBus(
        &self,
        _media_type: MediaType,
        _direction: BusDirection,
        _index: i32,
        _state: TBool,
    ) -> tresult {
        kResultOk
    }

    unsafe fn setActive(&self, active: TBool) -> tresult {
        let Ok(mut inner) = self.inner.lock() else {
            return kInternalError;
        };
        if active == 0 {
            inner.stash_and_clear();
            return kResultOk;
        }
        if inner.engines.is_empty() {
            let mut engines = Vec::with_capacity(self.models.len());
            for model in &self.models {
                let engine = match RackForgeEngine::open_plugin(
                    &model.plugin_id,
                    inner.sample_rate,
                    inner.maximum_frames,
                ) {
                    Ok(engine) => engine,
                    Err(error) => {
                        // A host reports this as one word -- Live says only
                        // "could not be activated (error: false)" -- so the
                        // reason has to be written down here or it is gone.
                        diagnostic::write(format!(
                            "setActive: opening {} at {} Hz, {} frames failed: {error:#}",
                            model.plugin_id, inner.sample_rate, inner.maximum_frames
                        ));
                        return kResultFalse;
                    }
                };
                engines.push(engine);
            }
            if engines.is_empty() {
                diagnostic::write(format!(
                    "setActive: no engines to open; {} bundled models",
                    self.models.len()
                ));
                return kResultFalse;
            }
            let pending_engine = inner.active_engine.min(engines.len() - 1);
            if !inner.pending_state.is_empty()
                && engines[pending_engine]
                    .load_state(&inner.pending_state)
                    .is_err()
            {
                return kResultFalse;
            }
            inner.engines = engines;
        }
        kResultOk
    }

    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        let Ok(bytes) = (unsafe { read_stream(stream) }) else {
            return kResultFalse;
        };
        let Ok(state) = decode_state(&bytes) else {
            return kResultFalse;
        };
        self.level.store(state.level.to_bits(), Ordering::Relaxed);
        let Ok(mut inner) = self.inner.lock() else {
            return kInternalError;
        };
        if let Some(plugin_id) = state.plugin_id {
            let Some(index) = self
                .models
                .iter()
                .position(|model| model.plugin_id == plugin_id)
            else {
                return kResultFalse;
            };
            inner.active_engine = index;
        }
        let active_engine = inner.active_engine;
        if let Some(engine) = inner.engines.get_mut(active_engine)
            && engine.load_state(state.plugin_state).is_err()
        {
            return kResultFalse;
        }
        inner.pending_state.clear();
        inner.pending_state.extend_from_slice(state.plugin_state);
        kResultOk
    }

    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        let Ok(mut inner) = self.inner.lock() else {
            return kInternalError;
        };
        let active_engine = inner.active_engine;
        let plugin_state = match inner.engines.get_mut(active_engine) {
            Some(engine) => match engine.save_state() {
                Ok(state) => state,
                Err(_) => return kResultFalse,
            },
            None => inner.pending_state.clone(),
        };
        let plugin_id = self
            .models
            .get(active_engine)
            .map(|model| model.plugin_id.as_str())
            .unwrap_or_default();
        let bytes = encode_state(self.level(), plugin_id, &plugin_state);
        if unsafe { write_stream(stream, &bytes) }.is_ok() {
            kResultOk
        } else {
            kResultFalse
        }
    }
}

impl IAudioProcessorTrait for RackForgeProcessor {
    unsafe fn setBusArrangements(
        &self,
        _inputs: *mut SpeakerArrangement,
        input_count: i32,
        outputs: *mut SpeakerArrangement,
        output_count: i32,
    ) -> tresult {
        if input_count != 0 || output_count != 1 || outputs.is_null() {
            return kResultFalse;
        }
        if unsafe { *outputs } == SpeakerArr::kStereo {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn getBusArrangement(
        &self,
        direction: BusDirection,
        index: i32,
        arrangement: *mut SpeakerArrangement,
    ) -> tresult {
        if arrangement.is_null()
            || direction as BusDirections != BusDirections_::kOutput
            || index != 0
        {
            return kInvalidArgument;
        }
        unsafe {
            *arrangement = SpeakerArr::kStereo;
        }
        kResultOk
    }

    unsafe fn canProcessSampleSize(&self, sample_size: i32) -> tresult {
        if sample_size as SymbolicSampleSizes == SymbolicSampleSizes_::kSample32 {
            kResultTrue
        } else {
            kResultFalse
        }
    }

    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }

    unsafe fn setupProcessing(&self, setup: *mut ProcessSetup) -> tresult {
        if setup.is_null() {
            return kInvalidArgument;
        }
        let setup = unsafe { &*setup };
        if !setup.sampleRate.is_finite() || setup.sampleRate <= 0.0 || setup.maxSamplesPerBlock <= 0
        {
            return kInvalidArgument;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return kInternalError;
        };
        inner.sample_rate = setup.sampleRate;
        inner.maximum_frames = setup.maxSamplesPerBlock as usize;
        // The rate and block size change for an offline render, and the
        // engines have to be rebuilt for them -- but not emptied of what the
        // player put in.
        inner.stash_and_clear();
        kResultOk
    }

    unsafe fn setProcessing(&self, _state: TBool) -> tresult {
        kResultOk
    }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        if data.is_null() {
            return kInvalidArgument;
        }
        let data = unsafe { &mut *data };
        if data.symbolicSampleSize as SymbolicSampleSizes != SymbolicSampleSizes_::kSample32
            || data.numSamples < 0
            || data.numOutputs != 1
            || data.outputs.is_null()
        {
            return kInvalidArgument;
        }
        update_level(data.inputParameterChanges, &self.level);
        let selected_engine = selected_plugin_index(data.inputParameterChanges, self.models.len());
        let frames = data.numSamples as usize;
        let output_bus = unsafe { &mut *data.outputs };
        if output_bus.numChannels != 2 {
            return kInvalidArgument;
        }
        let channels =
            unsafe { slice::from_raw_parts_mut(output_bus.__field0.channelBuffers32, 2) };
        if channels[0].is_null() || channels[1].is_null() {
            return kInvalidArgument;
        }
        let left = unsafe { slice::from_raw_parts_mut(channels[0], frames) };
        let right = unsafe { slice::from_raw_parts_mut(channels[1], frames) };
        left.fill(0.0);
        right.fill(0.0);

        // Never wait behind a project-state operation on the real-time thread.
        let Ok(mut inner) = self.inner.try_lock() else {
            return kResultOk;
        };
        if let Some(selected_engine) = selected_engine {
            inner.active_engine = selected_engine;
        }
        let active_engine = inner.active_engine;
        let Some(engine) = inner.engines.get_mut(active_engine) else {
            return kResultOk;
        };
        let events = VstMidiEvents::new(data.inputEvents, frames as u32).chain(
            VstControllerEvents::new(data.inputParameterChanges, frames as u32),
        );
        let parameter_events = VstPluginParameterEvents::new(
            data.inputParameterChanges,
            frames as u32,
            self.models.get(active_engine).map(Arc::as_ref),
        );
        if engine
            .process(
                frames,
                events,
                parameter_events,
                left,
                right,
                self.level() as f32,
            )
            .is_err()
        {
            left.fill(0.0);
            right.fill(0.0);
        }
        output_bus.silenceFlags =
            if left.iter().all(|value| *value == 0.0) && right.iter().all(|value| *value == 0.0) {
                0b11
            } else {
                0
            };
        kResultOk
    }

    unsafe fn getTailSamples(&self) -> u32 {
        kInfiniteTail
    }
}

impl IProcessContextRequirementsTrait for RackForgeProcessor {
    unsafe fn getProcessContextRequirements(&self) -> u32 {
        0
    }
}

struct VstMidiEvents {
    list: *mut IEventList,
    index: i32,
    count: i32,
    frames: u32,
}

impl VstMidiEvents {
    fn new(list: *mut IEventList, frames: u32) -> Self {
        let count = unsafe { ComRef::from_raw(list) }
            .map(|events| unsafe { events.getEventCount() })
            .unwrap_or(0);
        Self {
            list,
            index: 0,
            count,
            frames,
        }
    }
}

impl Iterator for VstMidiEvents {
    type Item = Midi2Event;

    fn next(&mut self) -> Option<Self::Item> {
        let list = unsafe { ComRef::from_raw(self.list) }?;
        while self.index < self.count {
            let index = self.index;
            self.index += 1;
            let mut event: Event = unsafe { std::mem::zeroed() };
            if unsafe { list.getEvent(index, &mut event) } != kResultOk {
                continue;
            }
            let frame = event.sampleOffset.max(0) as u32;
            let frame = frame.min(self.frames.saturating_sub(1));
            let converted = match event.r#type as Event_::EventTypes {
                Event_::EventTypes_::kNoteOnEvent => {
                    let note = unsafe { event.__field0.noteOn };
                    Some(midi_event(
                        frame,
                        0x90,
                        note.channel,
                        note.pitch,
                        note.velocity,
                    ))
                }
                Event_::EventTypes_::kNoteOffEvent => {
                    let note = unsafe { event.__field0.noteOff };
                    Some(midi_event(
                        frame,
                        0x80,
                        note.channel,
                        note.pitch,
                        note.velocity,
                    ))
                }
                Event_::EventTypes_::kPolyPressureEvent => {
                    let pressure = unsafe { event.__field0.polyPressure };
                    Some(midi_event(
                        frame,
                        0xA0,
                        pressure.channel,
                        pressure.pitch,
                        pressure.pressure,
                    ))
                }
                _ => None,
            };
            if converted.is_some() {
                return converted;
            }
        }
        None
    }
}

/// The byte a host's normalized value came from, if it came from one.
///
/// Every MIDI 1.0 source reaches a VST3 processor as `k / 127`, and a value
/// within a millionth of that grid is treated as that byte -- float error in
/// the host's division is a hundred times smaller. A value between grid
/// points can only have come from a source with more resolution than a
/// byte, and is kept whole.
fn seven_bit_origin(value: f64) -> Option<u8> {
    let value = value.clamp(0.0, 1.0);
    let byte = (value * 127.0).round();
    ((byte / 127.0 - value).abs() <= 1e-6).then_some(byte as u8)
}

/// A note event in the host's vocabulary.
///
/// This used to round the velocity to a byte, so a controller with more
/// than 128 levels, or a host that produced the note itself, was cut to MIDI
/// 1.0 before any instrument saw it. A seven-bit source now takes exactly
/// the path it took then -- the same three bytes, lifted, and flagged as
/// bytes so the instrument keeps its calibrated response for them -- and a
/// value off that grid carries its full width.
fn midi_event(frame: u32, status: u8, channel: i16, note: i16, value: f32) -> Midi2Event {
    let channel = channel.clamp(0, 15) as u8;
    let note = note.clamp(0, 127) as u8;
    let value = f64::from(value).clamp(0.0, 1.0);
    if let Some(byte) = seven_bit_origin(value) {
        return Midi2Event::from_midi1(&MidiEventV1 {
            frame,
            length: 3,
            data: [status | channel, note, byte],
        });
    }
    let message = match status {
        0x90 => Midi2Message::NoteOn {
            note,
            velocity: (value * 65535.0).round() as u16,
        },
        0x80 => Midi2Message::NoteOff {
            note,
            velocity: Some((value * 65535.0).round() as u16),
        },
        _ => Midi2Message::PolyPressure {
            note,
            pressure: (value * f64::from(u32::MAX)).round() as u32,
        },
    };
    Midi2Event {
        frame,
        channel,
        message,
        origin_7bit: false,
    }
}

/// A controller from the host's parameter queues, in the vocabulary.
///
/// A plain controller gets the same seven-bit test as a note: a byte stays
/// a byte, and a finer value -- a half-pedal drawn as an automation curve --
/// keeps its 32 bits. Aftertouch and pitch bend keep their byte forms.
fn controller_event(frame: u32, channel: u16, controller: u16, value: f64) -> Midi2Event {
    if controller < 128 && seven_bit_origin(value).is_none() {
        return Midi2Event {
            frame,
            channel: channel.min(15) as u8,
            message: Midi2Message::ControlChange {
                controller: controller as u8,
                value: (value.clamp(0.0, 1.0) * f64::from(u32::MAX)).round() as u32,
            },
            origin_7bit: false,
        };
    }
    Midi2Event::from_midi1(&controller_midi_event(frame, channel, controller, value))
}

#[derive(Clone)]
struct RackForgeControllerShared {
    level: Arc<AtomicU64>,
    revision: Arc<AtomicU64>,
    /// The revision of the last snapshot handed to the editor. A project's
    /// state may reach the controller after the editor has drawn its first
    /// one -- FL Studio reopens a project with its plug-in windows open -- and
    /// the editor asks for a snapshot only when it connects, so it went on
    /// showing the first instrument. A newer revision is now sent with the
    /// next meter reading, which the editor asks for continuously.
    published_revision: Arc<AtomicU64>,
    handler: Arc<Mutex<Option<ComPtr<IComponentHandler>>>>,
    model: Arc<RwLock<Option<Arc<VstPluginModel>>>>,
    catalog: Arc<Vec<Arc<VstPluginModel>>>,
    /// The process's one storage browser: see `engine::resource_browser`.
    ///
    /// `None` when it could not be opened -- a read-only or missing RackForge
    /// root. The config surface is then not offered at all rather than offered
    /// and broken: see `config_available`.
    /// Windows only, with the editor: the only reader is the web host that
    /// answers a config surface, and that is built for the WebView.
    #[cfg(windows)]
    resources: Option<Arc<rackforge_resource_host::NativeResourceBrowser>>,
    values: Arc<RwLock<BTreeMap<u32, f64>>>,
    selected_sound_id: Arc<RwLock<Option<String>>>,
    #[cfg(windows)]
    ui_route: Arc<RwLock<String>>,
}

impl RackForgeControllerShared {
    /// Asks the DAW to rebuild the audio side.
    ///
    /// The controller cannot reach the processor -- separate VST3 objects, and
    /// the processor owns the engines -- so an install cannot be pushed into
    /// the instrument that is sounding. It is written where the next instance
    /// will read it, and the host is asked to make a next instance now.
    /// Without this a cartridge would appear only whenever the DAW happened to
    /// reload the plug-in.
    #[cfg(windows)]
    fn reload_component(&self) {
        let Ok(handler) = self.handler.lock() else {
            return;
        };
        if let Some(handler) = handler.as_ref() {
            unsafe {
                let _ = handler.restartComponent(RestartFlags_::kReloadComponent);
            }
        }
    }

    fn model(&self) -> Option<Arc<VstPluginModel>> {
        self.model.read().ok()?.clone()
    }

    fn level(&self) -> f64 {
        f64::from_bits(self.level.load(Ordering::Relaxed)).clamp(0.0, 1.0)
    }

    fn set_level(&self, level: f64) {
        self.level
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(windows)]
    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    /// Whether the editor's last snapshot is older than the controller.
    #[cfg(windows)]
    fn editor_is_behind(&self) -> bool {
        self.published_revision.load(Ordering::Relaxed) != self.revision()
    }

    #[cfg(windows)]
    fn selected_sound_id(&self) -> Option<String> {
        self.selected_sound_id.read().ok()?.clone()
    }

    #[cfg(windows)]
    fn ui_route(&self) -> String {
        self.ui_route
            .read()
            .map(|route| route.clone())
            .unwrap_or_else(|_| "/".to_owned())
    }

    #[cfg(windows)]
    fn set_ui_route(&self, route: &str) -> Result<(), String> {
        if route.is_empty()
            || route.len() > 512
            || !route.starts_with('/')
            || route.contains(['#', '?', '\r', '\n'])
        {
            return Err("invalid RackForge VST3 UI route".to_owned());
        }
        *self
            .ui_route
            .write()
            .map_err(|_| "RackForge VST3 UI route is unavailable".to_owned())? = route.to_owned();
        Ok(())
    }

    #[cfg(windows)]
    fn editor_url(&self) -> String {
        format!("rackforge://localhost/index.html#{}", self.ui_route())
    }

    fn plugin_selection_normalized(&self) -> f64 {
        let Some(current) = self.model() else {
            return 0.0;
        };
        if self.catalog.len() <= 1 {
            return 0.0;
        }
        self.catalog
            .iter()
            .position(|model| model.plugin_id == current.plugin_id)
            .map(|index| index as f64 / (self.catalog.len() - 1) as f64)
            .unwrap_or(0.0)
    }

    #[cfg(windows)]
    fn set_level_from_ui(&self, level: f64) {
        if !level.is_finite() {
            return;
        }
        let level = level.clamp(0.0, 1.0);
        self.set_level(level);
        let Ok(handler) = self.handler.lock() else {
            return;
        };
        let Some(handler) = handler.as_ref() else {
            return;
        };
        unsafe {
            let _ = handler.beginEdit(MASTER_LEVEL);
            let _ = handler.performEdit(MASTER_LEVEL, level);
            let _ = handler.endEdit(MASTER_LEVEL);
        }
    }

    fn plugin_parameter_count(&self) -> usize {
        self.model()
            .as_deref()
            .map(|model| model.schema.parameters.len())
            .unwrap_or(0)
    }

    #[cfg(windows)]
    fn parameter(&self, index: u32) -> Option<ParameterDescriptor> {
        self.model()?
            .schema
            .parameters
            .iter()
            .find(|parameter| parameter.index == index)
            .cloned()
    }

    fn plugin_value(&self, index: u32) -> Option<f64> {
        self.values.read().ok()?.get(&index).copied()
    }

    #[cfg(windows)]
    fn set_plugin_parameter_from_ui(&self, index: u32, value: f64) -> Option<f64> {
        let parameter = self.parameter(index)?;
        if parameter.flags.read_only || matches!(parameter.kind, ParameterKind::Meter { .. }) {
            return None;
        }
        let normalized = parameter_plain_to_normalized(&parameter, value)?;
        let canonical = parameter_normalized_to_plain(&parameter, normalized);
        if let Ok(mut values) = self.values.write() {
            values.insert(index, canonical);
        }
        self.revision.fetch_add(1, Ordering::Relaxed);
        let handler = self.handler.lock().ok()?;
        let handler = handler.as_ref()?;
        let parameter_id = plugin_parameter_id(index)?;
        unsafe {
            let _ = handler.beginEdit(parameter_id);
            let _ = handler.performEdit(parameter_id, normalized);
            let _ = handler.endEdit(parameter_id);
        }
        Some(canonical)
    }

    #[cfg(windows)]
    fn apply_preset_from_ui(&self, preset_id: &str) -> Option<Vec<engine::VstParameterValue>> {
        let values = self.model()?.preset_values.get(preset_id)?.clone();
        for value in &values {
            if self.parameter(value.index).is_some_and(|parameter| {
                !parameter.flags.read_only && !matches!(parameter.kind, ParameterKind::Meter { .. })
            }) {
                let _ = self.set_plugin_parameter_from_ui(value.index, value.value);
            }
        }
        if let Ok(mut selected) = self.selected_sound_id.write() {
            *selected = Some(preset_id.to_owned());
        }
        self.revision.fetch_add(1, Ordering::Relaxed);
        Some(values)
    }

    fn apply_plugin_selection(&self, catalog_index: usize) -> Result<Arc<VstPluginModel>, String> {
        let model = self
            .catalog
            .get(catalog_index)
            .cloned()
            .ok_or_else(|| format!("VST3 plugin index {catalog_index} is unavailable"))?;
        if let Ok(mut values) = self.values.write() {
            values.clear();
            values.extend(
                model
                    .initial_values
                    .iter()
                    .map(|value| (value.index, value.value)),
            );
        }
        if let Ok(mut selected) = self.selected_sound_id.write() {
            *selected = model.initial_sound_id.clone();
        }
        if let Ok(mut current) = self.model.write() {
            *current = Some(model.clone());
        }
        self.revision.fetch_add(1, Ordering::Relaxed);
        Ok(model)
    }

    /// The instrument a saved project names, from either half of its state.
    ///
    /// Selected afresh only if it is not the one already shown: a host may
    /// hand back the controller's own state, with the program and values the
    /// editor showed, before the processor's, and selecting the same
    /// instrument again would put both back to its defaults.
    fn restore_plugin(&self, plugin_id: &str) -> Result<(), String> {
        if self
            .model()
            .is_some_and(|current| current.plugin_id == plugin_id)
        {
            return Ok(());
        }
        let index = self
            .catalog
            .iter()
            .position(|model| model.plugin_id == plugin_id)
            .ok_or_else(|| format!("Plugin {plugin_id} is not bundled with RackForge VST3"))?;
        self.apply_plugin_selection(index).map(|_| ())
    }

    /// The program and parameter values the editor showed when the project
    /// was saved, over the restored instrument's defaults.
    fn restore_editor(&self, sound_id: Option<String>, values: &[(u32, f64)]) {
        if !values.is_empty()
            && let Ok(mut current) = self.values.write()
        {
            current.extend(values.iter().copied());
        }
        if let Some(sound_id) = sound_id
            && self
                .model()
                .is_some_and(|model| model.preset_names.contains_key(&sound_id))
            && let Ok(mut selected) = self.selected_sound_id.write()
        {
            *selected = Some(sound_id);
        }
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    #[cfg(windows)]
    fn select_plugin_from_ui(&self, plugin_id: &str) -> Result<Arc<VstPluginModel>, String> {
        let catalog_index = self
            .catalog
            .iter()
            .position(|model| model.plugin_id == plugin_id)
            .ok_or_else(|| format!("Plugin {plugin_id} is not bundled with RackForge VST3"))?;
        let model = self.apply_plugin_selection(catalog_index)?;
        let handler = self
            .handler
            .lock()
            .map_err(|_| "VST3 component handler is unavailable".to_owned())?;
        if let Some(handler) = handler.as_ref() {
            let normalized = if self.catalog.len() <= 1 {
                0.0
            } else {
                catalog_index as f64 / (self.catalog.len() - 1) as f64
            };
            unsafe {
                let _ = handler.beginEdit(PLUGIN_SELECTION);
                let _ = handler.performEdit(PLUGIN_SELECTION, normalized);
                let _ = handler.endEdit(PLUGIN_SELECTION);
                let _ = handler.restartComponent(
                    RestartFlags_::kParamTitlesChanged | RestartFlags_::kParamValuesChanged,
                );
            }
        }
        Ok(model)
    }
}

struct RackForgeController {
    shared: RackForgeControllerShared,
}

impl Class for RackForgeController {
    type Interfaces = (IEditController, IMidiMapping);
}

impl RackForgeController {
    const CID: TUID = uid(0xA9E488B2, 0xF36E4B52, 0xB1FD8D9B, 0xAF4016CC);
    fn new() -> Self {
        let catalog: Vec<Arc<VstPluginModel>> = engine::load_bundled_plugin_models()
            .map(|models| models.into_iter().map(Arc::new).collect())
            .inspect_err(|error| {
                diagnostic::write(format!("controller plugin models unavailable: {error:#}"));
            })
            .unwrap_or_default();
        let model = catalog.first().cloned();
        let values = model
            .as_ref()
            .map(|model| {
                model
                    .initial_values
                    .iter()
                    .map(|value| (value.index, value.value))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            shared: RackForgeControllerShared {
                level: Arc::new(AtomicU64::new(1.0_f64.to_bits())),
                revision: Arc::new(AtomicU64::new(0)),
                published_revision: Arc::new(AtomicU64::new(0)),
                handler: Arc::new(Mutex::new(None)),
                catalog: Arc::new(catalog),
                #[cfg(windows)]
                resources: engine::resource_browser(),
                selected_sound_id: Arc::new(RwLock::new(
                    model
                        .as_ref()
                        .and_then(|model| model.initial_sound_id.clone()),
                )),
                #[cfg(windows)]
                ui_route: Arc::new(RwLock::new("/".to_owned())),
                model: Arc::new(RwLock::new(model)),
                values: Arc::new(RwLock::new(values)),
            },
        }
    }
}

impl IPluginBaseTrait for RackForgeController {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult {
        kResultOk
    }
    unsafe fn terminate(&self) -> tresult {
        kResultOk
    }
}

impl IEditControllerTrait for RackForgeController {
    unsafe fn setComponentState(&self, stream: *mut IBStream) -> tresult {
        let Ok(bytes) = (unsafe { read_stream(stream) }) else {
            return kResultFalse;
        };
        let Ok(state) = decode_state(&bytes) else {
            return kResultFalse;
        };
        diagnostic::write(format!(
            "controller.setComponentState plugin={:?} current={:?}",
            state.plugin_id,
            self.shared.model().map(|model| model.plugin_id.clone())
        ));
        if let Some(plugin_id) = state.plugin_id
            && self.shared.restore_plugin(plugin_id).is_err()
        {
            return kResultFalse;
        }
        self.shared.set_level(state.level);
        kResultOk
    }
    unsafe fn setState(&self, stream: *mut IBStream) -> tresult {
        let Ok(bytes) = (unsafe { read_stream(stream) }) else {
            return kResultFalse;
        };
        let Ok(state) = decode_controller_state(&bytes) else {
            diagnostic::write(format!(
                "controller.setState unreadable, {} bytes",
                bytes.len()
            ));
            return kResultFalse;
        };
        diagnostic::write(format!(
            "controller.setState plugin={:?} sound={:?} values={}",
            state.plugin_id,
            state.sound_id,
            state.values.len()
        ));
        self.shared.set_level(state.level);
        if let Some(plugin_id) = &state.plugin_id {
            if self.shared.restore_plugin(plugin_id).is_err() {
                return kResultFalse;
            }
            self.shared.restore_editor(state.sound_id, &state.values);
        }
        kResultOk
    }
    unsafe fn getState(&self, stream: *mut IBStream) -> tresult {
        let model = self.shared.model();
        let values = self
            .shared
            .values
            .read()
            .map(|values| values.clone())
            .unwrap_or_default();
        let bytes = encode_controller_state(
            self.shared.level(),
            model.as_ref().map(|model| model.plugin_id.as_str()),
            self.shared
                .selected_sound_id
                .read()
                .ok()
                .and_then(|id| id.clone())
                .as_deref(),
            &values,
        );
        if unsafe { write_stream(stream, &bytes) }.is_ok() {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn getParameterCount(&self) -> i32 {
        (2 + self.shared.plugin_parameter_count() as u32 + MIDI_PARAMETER_COUNT) as i32
    }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let plugin_count = self.shared.plugin_parameter_count();
        let total = 2 + plugin_count + MIDI_PARAMETER_COUNT as usize;
        if index < 0 || index as usize >= total || info.is_null() {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        if index == 0 {
            info.id = MASTER_LEVEL;
            copy_wstring("Master Level", &mut info.title);
            copy_wstring("Level", &mut info.shortTitle);
            copy_wstring("%", &mut info.units);
            info.stepCount = 0;
            info.defaultNormalizedValue = 1.0;
            info.unitId = kRootUnitId;
            info.flags = ParameterInfo_::ParameterFlags_::kCanAutomate;
        } else if index == 1 {
            info.id = PLUGIN_SELECTION;
            copy_wstring("RackForge Instrument", &mut info.title);
            copy_wstring("Instrument", &mut info.shortTitle);
            copy_wstring("", &mut info.units);
            info.stepCount = self.shared.catalog.len().saturating_sub(1) as i32;
            info.defaultNormalizedValue = 0.0;
            info.unitId = kRootUnitId;
            info.flags = ParameterInfo_::ParameterFlags_::kIsHidden;
        } else if (index as usize) <= plugin_count + 1 {
            let Some(model) = self.shared.model() else {
                return kInvalidArgument;
            };
            let Some(parameter) = model.schema.parameters.get(index as usize - 2) else {
                return kInvalidArgument;
            };
            let Some(id) = plugin_parameter_id(parameter.index) else {
                return kInvalidArgument;
            };
            info.id = id;
            copy_wstring(&parameter.name, &mut info.title);
            copy_wstring(&parameter.name, &mut info.shortTitle);
            copy_wstring(parameter_unit(&parameter.kind), &mut info.units);
            info.stepCount = parameter_step_count(&parameter.kind);
            info.defaultNormalizedValue = parameter_default_normalized(parameter);
            info.unitId = kRootUnitId;
            info.flags = if parameter.flags.automatable && !parameter.flags.read_only {
                ParameterInfo_::ParameterFlags_::kCanAutomate
            } else {
                ParameterInfo_::ParameterFlags_::kIsReadOnly
            };
        } else {
            let midi_index = index as usize - 2 - plugin_count;
            let (channel, controller) = midi_parameter_from_index(midi_index as u32);
            info.id = midi_parameter_id(channel, controller);
            let label = midi_controller_label(channel, controller);
            copy_wstring(&label, &mut info.title);
            copy_wstring(&label, &mut info.shortTitle);
            copy_wstring("", &mut info.units);
            info.stepCount = if controller == ControllerNumbers_::kPitchBend as u16 {
                16_383
            } else {
                127
            };
            info.defaultNormalizedValue = if controller == ControllerNumbers_::kPitchBend as u16 {
                0.5
            } else {
                0.0
            };
            info.unitId = kRootUnitId;
            info.flags = ParameterInfo_::ParameterFlags_::kIsHidden;
        }
        kResultOk
    }
    unsafe fn getParamStringByValue(&self, id: u32, value: f64, text: *mut String128) -> tresult {
        if text.is_null() {
            return kInvalidArgument;
        }
        let model = self.shared.model();
        let rendered = if id == MASTER_LEVEL {
            format!("{:.0}", value.clamp(0.0, 1.0) * 100.0)
        } else if id == PLUGIN_SELECTION {
            let index = (value.clamp(0.0, 1.0) * self.shared.catalog.len().saturating_sub(1) as f64)
                .round() as usize;
            self.shared
                .catalog
                .get(index)
                .map(|model| model.name.clone())
                .unwrap_or_else(|| "Instrument".to_owned())
        } else if let Some(parameter) = decode_plugin_parameter(model.as_deref(), id) {
            format_parameter_value(parameter, parameter_normalized_to_plain(parameter, value))
        } else if decode_midi_parameter(id).is_some() {
            format!("{:.0}", value.clamp(0.0, 1.0) * 127.0)
        } else {
            return kInvalidArgument;
        };
        copy_wstring(&rendered, unsafe { &mut *text });
        kResultOk
    }
    unsafe fn getParamValueByString(&self, id: u32, text: *mut TChar, value: *mut f64) -> tresult {
        if text.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        let length = unsafe { len_wstring(text) };
        let Ok(text) = String::from_utf16(unsafe { slice::from_raw_parts(text, length) }) else {
            return kInvalidArgument;
        };
        let Ok(parsed) = f64::from_str(text.trim_end_matches('%').trim()) else {
            return kInvalidArgument;
        };
        let model = self.shared.model();
        let normalized = if id == MASTER_LEVEL {
            if parsed > 1.0 { parsed / 100.0 } else { parsed }.clamp(0.0, 1.0)
        } else if id == PLUGIN_SELECTION {
            parsed.clamp(0.0, 1.0)
        } else if let Some(parameter) = decode_plugin_parameter(model.as_deref(), id) {
            let Some(normalized) = parameter_plain_to_normalized(parameter, parsed) else {
                return kInvalidArgument;
            };
            normalized
        } else if decode_midi_parameter(id).is_some() {
            if parsed > 1.0 { parsed / 127.0 } else { parsed }.clamp(0.0, 1.0)
        } else {
            return kInvalidArgument;
        };
        unsafe { *value = normalized };
        kResultOk
    }
    unsafe fn normalizedParamToPlain(&self, id: u32, value: f64) -> f64 {
        let model = self.shared.model();
        if id == MASTER_LEVEL || id == PLUGIN_SELECTION || decode_midi_parameter(id).is_some() {
            value.clamp(0.0, 1.0)
        } else if let Some(parameter) = decode_plugin_parameter(model.as_deref(), id) {
            parameter_normalized_to_plain(parameter, value)
        } else {
            0.0
        }
    }
    unsafe fn plainParamToNormalized(&self, id: u32, value: f64) -> f64 {
        let model = self.shared.model();
        if id == MASTER_LEVEL || id == PLUGIN_SELECTION || decode_midi_parameter(id).is_some() {
            value.clamp(0.0, 1.0)
        } else if let Some(parameter) = decode_plugin_parameter(model.as_deref(), id) {
            parameter_plain_to_normalized(parameter, value).unwrap_or(0.0)
        } else {
            0.0
        }
    }
    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        let model = self.shared.model();
        if id == MASTER_LEVEL {
            self.shared.level()
        } else if id == PLUGIN_SELECTION {
            self.shared.plugin_selection_normalized()
        } else if let Some(parameter) = decode_plugin_parameter(model.as_deref(), id) {
            self.shared
                .plugin_value(parameter.index)
                .and_then(|value| parameter_plain_to_normalized(parameter, value))
                .unwrap_or_else(|| parameter_default_normalized(parameter))
        } else if let Some((_, controller)) = decode_midi_parameter(id) {
            if controller == ControllerNumbers_::kPitchBend as u16 {
                0.5
            } else {
                0.0
            }
        } else {
            0.0
        }
    }
    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        if !value.is_finite() {
            return kInvalidArgument;
        }
        let model = self.shared.model();
        if id == MASTER_LEVEL {
            self.shared.set_level(value);
        } else if id == PLUGIN_SELECTION {
            let index = (value.clamp(0.0, 1.0) * self.shared.catalog.len().saturating_sub(1) as f64)
                .round() as usize;
            if self.shared.catalog.get(index).is_none() {
                return kInvalidArgument;
            }
            if self.shared.apply_plugin_selection(index).is_err() {
                return kResultFalse;
            }
        } else if let Some(parameter) = decode_plugin_parameter(model.as_deref(), id) {
            if parameter.flags.read_only || matches!(parameter.kind, ParameterKind::Meter { .. }) {
                return kInvalidArgument;
            }
            if let Ok(mut values) = self.shared.values.write() {
                values.insert(
                    parameter.index,
                    parameter_normalized_to_plain(parameter, value),
                );
            }
        } else if decode_midi_parameter(id).is_none() {
            return kInvalidArgument;
        }
        kResultOk
    }
    unsafe fn setComponentHandler(&self, handler: *mut IComponentHandler) -> tresult {
        let owned = unsafe { ComRef::from_raw(handler) }.map(|handler| handler.to_com_ptr());
        let Ok(mut current) = self.shared.handler.lock() else {
            return kInternalError;
        };
        *current = owned;
        kResultOk
    }
    unsafe fn createView(&self, name: *const c_char) -> *mut IPlugView {
        if name.is_null() {
            diagnostic::write("controller.createView rejected null view name");
            return ptr::null_mut();
        }
        let name = unsafe { CStr::from_ptr(name) }.to_string_lossy();
        diagnostic::write(format!("controller.createView name={name:?}"));
        if name.as_bytes() != b"editor" {
            diagnostic::write("controller.createView rejected unsupported view name");
            return ptr::null_mut();
        }
        let result = ComWrapper::new(view::RackForgeView::new(self.shared.clone()))
            .to_com_ptr::<IPlugView>()
            .map(ComPtr::into_raw)
            .unwrap_or(ptr::null_mut());
        diagnostic::write(format!(
            "controller.createView result={}",
            if result.is_null() { "null" } else { "ok" }
        ));
        result
    }
}

impl IMidiMappingTrait for RackForgeController {
    unsafe fn getMidiControllerAssignment(
        &self,
        bus_index: i32,
        channel: i16,
        controller: CtrlNumber,
        id: *mut ParamID,
    ) -> tresult {
        if bus_index != 0
            || !(0..MIDI_CHANNELS as i16).contains(&channel)
            || !(0..MIDI_CONTROLLERS_PER_CHANNEL as i16).contains(&controller)
            || id.is_null()
        {
            return kInvalidArgument;
        }
        unsafe {
            *id = midi_parameter_id(channel as u16, controller as u16);
        }
        kResultTrue
    }
}

struct VstControllerEvents {
    changes: *mut IParameterChanges,
    queue_index: i32,
    queue_count: i32,
    point_index: i32,
    point_count: i32,
    queue: *mut IParamValueQueue,
    mapping: Option<(u16, u16)>,
    frames: u32,
}

impl VstControllerEvents {
    fn new(changes: *mut IParameterChanges, frames: u32) -> Self {
        let queue_count = unsafe { ComRef::from_raw(changes) }
            .map(|changes| unsafe { changes.getParameterCount() })
            .unwrap_or(0);
        Self {
            changes,
            queue_index: 0,
            queue_count,
            point_index: 0,
            point_count: 0,
            queue: ptr::null_mut(),
            mapping: None,
            frames,
        }
    }
}

impl Iterator for VstControllerEvents {
    type Item = Midi2Event;

    fn next(&mut self) -> Option<Self::Item> {
        let changes = unsafe { ComRef::from_raw(self.changes) }?;
        loop {
            if !self.queue.is_null() && self.point_index < self.point_count {
                let queue = unsafe { ComRef::from_raw(self.queue) }?;
                let mut frame = 0;
                let mut value = 0.0;
                let point = self.point_index;
                self.point_index += 1;
                if unsafe { queue.getPoint(point, &mut frame, &mut value) } != kResultTrue
                    || !value.is_finite()
                {
                    continue;
                }
                let (channel, controller) = self.mapping?;
                return Some(controller_event(
                    (frame.max(0) as u32).min(self.frames.saturating_sub(1)),
                    channel,
                    controller,
                    value,
                ));
            }
            if self.queue_index >= self.queue_count {
                return None;
            }
            self.queue = unsafe { changes.getParameterData(self.queue_index) };
            self.queue_index += 1;
            let Some(queue) = (unsafe { ComRef::from_raw(self.queue) }) else {
                continue;
            };
            self.mapping = decode_midi_parameter(unsafe { queue.getParameterId() });
            self.point_index = 0;
            self.point_count = unsafe { queue.getPointCount() };
            if self.mapping.is_none() {
                self.point_index = self.point_count;
            }
        }
    }
}

struct VstPluginParameterEvents<'a> {
    changes: *mut IParameterChanges,
    queue_index: i32,
    queue_count: i32,
    point_index: i32,
    point_count: i32,
    queue: *mut IParamValueQueue,
    parameter: Option<&'a ParameterDescriptor>,
    model: Option<&'a VstPluginModel>,
    frames: u32,
}

impl<'a> VstPluginParameterEvents<'a> {
    fn new(
        changes: *mut IParameterChanges,
        frames: u32,
        model: Option<&'a VstPluginModel>,
    ) -> Self {
        let queue_count = unsafe { ComRef::from_raw(changes) }
            .map(|changes| unsafe { changes.getParameterCount() })
            .unwrap_or(0);
        Self {
            changes,
            queue_index: 0,
            queue_count,
            point_index: 0,
            point_count: 0,
            queue: ptr::null_mut(),
            parameter: None,
            model,
            frames,
        }
    }
}

impl Iterator for VstPluginParameterEvents<'_> {
    type Item = ParameterEventV1;

    fn next(&mut self) -> Option<Self::Item> {
        let changes = unsafe { ComRef::from_raw(self.changes) }?;
        loop {
            if !self.queue.is_null() && self.point_index < self.point_count {
                let queue = unsafe { ComRef::from_raw(self.queue) }?;
                let mut frame = 0;
                let mut normalized = 0.0;
                let point = self.point_index;
                self.point_index += 1;
                if unsafe { queue.getPoint(point, &mut frame, &mut normalized) } != kResultTrue
                    || !normalized.is_finite()
                {
                    continue;
                }
                let parameter = self.parameter?;
                return Some(ParameterEventV1 {
                    frame: (frame.max(0) as u32).min(self.frames.saturating_sub(1)),
                    parameter_index: parameter.index,
                    value: parameter_normalized_to_plain(parameter, normalized),
                });
            }
            if self.queue_index >= self.queue_count {
                return None;
            }
            self.queue = unsafe { changes.getParameterData(self.queue_index) };
            self.queue_index += 1;
            let Some(queue) = (unsafe { ComRef::from_raw(self.queue) }) else {
                continue;
            };
            self.parameter = decode_plugin_parameter(self.model, unsafe { queue.getParameterId() })
                .filter(|parameter| {
                    !parameter.flags.read_only
                        && !matches!(parameter.kind, ParameterKind::Meter { .. })
                });
            self.point_index = 0;
            self.point_count = unsafe { queue.getPointCount() };
            if self.parameter.is_none() {
                self.point_index = self.point_count;
            }
        }
    }
}

fn plugin_parameter_id(index: u32) -> Option<ParamID> {
    PLUGIN_PARAMETER_BASE.checked_add(index)
}

fn decode_plugin_parameter(
    model: Option<&VstPluginModel>,
    id: ParamID,
) -> Option<&ParameterDescriptor> {
    let index = id.checked_sub(PLUGIN_PARAMETER_BASE)?;
    model?
        .schema
        .parameters
        .iter()
        .find(|parameter| parameter.index == index)
}

fn parameter_default_normalized(parameter: &ParameterDescriptor) -> f64 {
    let value = match &parameter.kind {
        ParameterKind::Float { default, .. } => *default,
        ParameterKind::Integer { default, .. } => *default as f64,
        ParameterKind::Boolean { default } => f64::from(*default),
        ParameterKind::Enum { default, .. } => *default as f64,
        ParameterKind::Trigger => 0.0,
        ParameterKind::Meter { minimum, .. } => *minimum,
    };
    parameter_plain_to_normalized(parameter, value).unwrap_or(0.0)
}

fn parameter_plain_to_normalized(parameter: &ParameterDescriptor, value: f64) -> Option<f64> {
    if !value.is_finite() {
        return None;
    }
    match &parameter.kind {
        ParameterKind::Float {
            minimum,
            maximum,
            taper,
            ..
        } if (*minimum..=*maximum).contains(&value) => {
            Some(if *taper == ParameterTaper::Logarithmic && *minimum > 0.0 {
                (value / *minimum).ln() / (*maximum / *minimum).ln()
            } else {
                (value - *minimum) / (*maximum - *minimum)
            })
        }
        ParameterKind::Meter {
            minimum, maximum, ..
        } if (*minimum..=*maximum).contains(&value) => {
            Some((value - *minimum) / (*maximum - *minimum))
        }
        ParameterKind::Integer {
            minimum,
            maximum,
            step,
            ..
        } if value >= *minimum as f64
            && value <= *maximum as f64
            && ((value - *minimum as f64) / *step as f64).fract().abs() < 1e-7 =>
        {
            Some((value - *minimum as f64) / (*maximum - *minimum) as f64)
        }
        ParameterKind::Boolean { .. } | ParameterKind::Trigger if value == 0.0 || value == 1.0 => {
            Some(value)
        }
        ParameterKind::Enum { choices, .. } => {
            let position = choices
                .iter()
                .position(|choice| choice.value as f64 == value)?;
            Some(if choices.len() <= 1 {
                0.0
            } else {
                position as f64 / (choices.len() - 1) as f64
            })
        }
        _ => None,
    }
}

fn parameter_normalized_to_plain(parameter: &ParameterDescriptor, normalized: f64) -> f64 {
    let normalized = normalized.clamp(0.0, 1.0);
    match &parameter.kind {
        ParameterKind::Float {
            minimum,
            maximum,
            step,
            taper,
            ..
        } => {
            // A host automates in nought to one, and the parameter says what
            // a position in that means. Spread linearly, a control whose
            // range is a sixteenth of its value to sixteen times it sits at
            // 0.06 of the host's fader, and the whole useful half of it is in
            // the first breath of travel. The taper is the one the panel and
            // the little screen read, and `parameter_plain_to_normalized` is
            // its exact inverse, so a value written back is the value read.
            if *taper == ParameterTaper::Logarithmic && *minimum > 0.0 {
                (*minimum * (*maximum / *minimum).powf(normalized)).clamp(*minimum, *maximum)
            } else {
                let raw = *minimum + (*maximum - *minimum) * normalized;
                let steps = ((raw - *minimum) / *step).round();
                (*minimum + steps * *step).clamp(*minimum, *maximum)
            }
        }
        ParameterKind::Integer {
            minimum,
            maximum,
            step,
            ..
        } => {
            let raw = *minimum as f64 + (*maximum - *minimum) as f64 * normalized;
            let steps = ((raw - *minimum as f64) / *step as f64).round();
            (*minimum as f64 + steps * *step as f64).clamp(*minimum as f64, *maximum as f64)
        }
        ParameterKind::Boolean { .. } | ParameterKind::Trigger => f64::from(normalized >= 0.5),
        ParameterKind::Enum { choices, .. } => {
            let index = (normalized * choices.len().saturating_sub(1) as f64).round() as usize;
            choices
                .get(index)
                .map(|choice| choice.value as f64)
                .unwrap_or(0.0)
        }
        ParameterKind::Meter {
            minimum, maximum, ..
        } => *minimum + (*maximum - *minimum) * normalized,
    }
}

fn parameter_step_count(kind: &ParameterKind) -> i32 {
    match kind {
        ParameterKind::Float { .. } | ParameterKind::Meter { .. } => 0,
        ParameterKind::Integer {
            minimum,
            maximum,
            step,
            ..
        } => ((*maximum - *minimum) / *step).clamp(1, i32::MAX as i64) as i32,
        ParameterKind::Boolean { .. } | ParameterKind::Trigger => 1,
        ParameterKind::Enum { choices, .. } => choices.len().saturating_sub(1) as i32,
    }
}

fn parameter_unit(kind: &ParameterKind) -> &str {
    match kind {
        ParameterKind::Float { unit, .. } | ParameterKind::Meter { unit, .. } => {
            unit.as_deref().unwrap_or("")
        }
        ParameterKind::Integer { unit, .. } => unit.as_deref().unwrap_or(""),
        _ => "",
    }
}

fn format_parameter_value(parameter: &ParameterDescriptor, value: f64) -> String {
    match &parameter.kind {
        ParameterKind::Boolean { .. } | ParameterKind::Trigger => {
            if value >= 0.5 { "On" } else { "Off" }.to_owned()
        }
        ParameterKind::Enum { choices, .. } => choices
            .iter()
            .find(|choice| choice.value as f64 == value)
            .map(|choice| choice.name.clone())
            .unwrap_or_else(|| format!("{value:.0}")),
        ParameterKind::Integer { .. } => format!("{value:.0}"),
        _ => format!("{value:.3}"),
    }
}

fn midi_parameter_id(channel: u16, controller: u16) -> ParamID {
    MIDI_PARAMETER_BASE + channel as u32 * MIDI_CONTROLLERS_PER_CHANNEL + controller as u32
}

fn decode_midi_parameter(id: ParamID) -> Option<(u16, u16)> {
    let offset = id.checked_sub(MIDI_PARAMETER_BASE)?;
    if offset >= MIDI_PARAMETER_COUNT {
        return None;
    }
    Some((
        (offset / MIDI_CONTROLLERS_PER_CHANNEL) as u16,
        (offset % MIDI_CONTROLLERS_PER_CHANNEL) as u16,
    ))
}

fn midi_parameter_from_index(index: u32) -> (u16, u16) {
    (
        (index / MIDI_CONTROLLERS_PER_CHANNEL) as u16,
        (index % MIDI_CONTROLLERS_PER_CHANNEL) as u16,
    )
}

fn midi_controller_label(channel: u16, controller: u16) -> String {
    match controller {
        value if value == ControllerNumbers_::kAfterTouch as u16 => {
            format!("MIDI Ch {} Pressure", channel + 1)
        }
        value if value == ControllerNumbers_::kPitchBend as u16 => {
            format!("MIDI Ch {} Pitch Bend", channel + 1)
        }
        _ => format!("MIDI Ch {} CC {}", channel + 1, controller),
    }
}

fn controller_midi_event(frame: u32, channel: u16, controller: u16, value: f64) -> MidiEventV1 {
    let channel = channel.min(15) as u8;
    let normalized = value.clamp(0.0, 1.0);
    match controller {
        value if value == ControllerNumbers_::kAfterTouch as u16 => MidiEventV1 {
            frame,
            length: 2,
            data: [0xD0 | channel, (normalized * 127.0).round() as u8, 0],
        },
        value if value == ControllerNumbers_::kPitchBend as u16 => {
            let bend = (normalized * 16_383.0).round() as u16;
            MidiEventV1 {
                frame,
                length: 3,
                data: [0xE0 | channel, (bend & 0x7f) as u8, (bend >> 7) as u8],
            }
        }
        _ => MidiEventV1 {
            frame,
            length: 3,
            data: [
                0xB0 | channel,
                controller.min(127) as u8,
                (normalized * 127.0).round() as u8,
            ],
        },
    }
}

struct Factory;
impl Class for Factory {
    type Interfaces = (IPluginFactory2,);
}

impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        copy_cstring("RackForge", &mut info.vendor);
        copy_cstring("https://github.com/kalexis1994/rackforge", &mut info.url);
        copy_cstring("", &mut info.email);
        info.flags = vst3_enum_i32(PFactoryInfo_::FactoryFlags_::kUnicode);
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 {
        2
    }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        match index {
            0 => {
                info.cid = RackForgeProcessor::CID;
                info.cardinality = vst3_enum_i32(PClassInfo_::ClassCardinality_::kManyInstances);
                copy_cstring("Audio Module Class", &mut info.category);
            }
            1 => {
                info.cid = RackForgeController::CID;
                info.cardinality = vst3_enum_i32(PClassInfo_::ClassCardinality_::kManyInstances);
                copy_cstring("Component Controller Class", &mut info.category);
            }
            _ => return kInvalidArgument,
        }
        copy_cstring(PLUGIN_NAME, &mut info.name);
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        cid: FIDString,
        iid: FIDString,
        object: *mut *mut c_void,
    ) -> tresult {
        if cid.is_null() || iid.is_null() || object.is_null() {
            return kInvalidArgument;
        }
        let instance = match unsafe { *(cid as *const TUID) } {
            RackForgeProcessor::CID => {
                diagnostic::write("factory.createInstance processor");
                ComWrapper::new(RackForgeProcessor::new())
                    .to_com_ptr::<FUnknown>()
                    .unwrap()
            }
            RackForgeController::CID => {
                diagnostic::write("factory.createInstance controller");
                ComWrapper::new(RackForgeController::new())
                    .to_com_ptr::<FUnknown>()
                    .unwrap()
            }
            _ => return kInvalidArgument,
        };
        let raw = instance.as_ptr();
        unsafe { ((*(*raw).vtbl).queryInterface)(raw, iid as *mut TUID, object) }
    }
}

impl IPluginFactory2Trait for Factory {
    unsafe fn getClassInfo2(&self, index: i32, info: *mut PClassInfo2) -> tresult {
        if info.is_null() {
            return kInvalidArgument;
        }
        let info = unsafe { &mut *info };
        match index {
            0 => {
                info.cid = RackForgeProcessor::CID;
                info.cardinality = vst3_enum_i32(PClassInfo_::ClassCardinality_::kManyInstances);
                copy_cstring("Audio Module Class", &mut info.category);
                copy_cstring("Instrument|Synth", &mut info.subCategories);
            }
            1 => {
                info.cid = RackForgeController::CID;
                info.cardinality = vst3_enum_i32(PClassInfo_::ClassCardinality_::kManyInstances);
                copy_cstring("Component Controller Class", &mut info.category);
                copy_cstring("", &mut info.subCategories);
            }
            _ => return kInvalidArgument,
        }
        copy_cstring(PLUGIN_NAME, &mut info.name);
        info.classFlags = 0;
        copy_cstring("RackForge", &mut info.vendor);
        copy_cstring(env!("CARGO_PKG_VERSION"), &mut info.version);
        copy_cstring("VST 3.8", &mut info.sdkVersion);
        kResultOk
    }
}

fn update_level(changes: *mut IParameterChanges, level: &AtomicU64) {
    let Some(changes) = (unsafe { ComRef::from_raw(changes) }) else {
        return;
    };
    let count = unsafe { changes.getParameterCount() };
    for index in 0..count {
        let Some(queue) = (unsafe { ComRef::from_raw(changes.getParameterData(index)) }) else {
            continue;
        };
        if unsafe { queue.getParameterId() } != MASTER_LEVEL {
            continue;
        }
        let point_count = unsafe { queue.getPointCount() };
        if point_count <= 0 {
            continue;
        }
        let (mut offset, mut value) = (0, 0.0);
        if unsafe { queue.getPoint(point_count - 1, &mut offset, &mut value) } == kResultTrue
            && value.is_finite()
        {
            level.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }
}

fn selected_plugin_index(changes: *mut IParameterChanges, plugin_count: usize) -> Option<usize> {
    if plugin_count < 2 {
        return None;
    }
    let changes = unsafe { ComRef::from_raw(changes) }?;
    let count = unsafe { changes.getParameterCount() };
    for index in 0..count {
        let Some(queue) = (unsafe { ComRef::from_raw(changes.getParameterData(index)) }) else {
            continue;
        };
        if unsafe { queue.getParameterId() } != PLUGIN_SELECTION {
            continue;
        }
        let point_count = unsafe { queue.getPointCount() };
        if point_count <= 0 {
            continue;
        }
        let (mut offset, mut value) = (0, 0.0);
        if unsafe { queue.getPoint(point_count - 1, &mut offset, &mut value) } == kResultTrue
            && value.is_finite()
        {
            return Some((value.clamp(0.0, 1.0) * (plugin_count - 1) as f64).round() as usize);
        }
    }
    None
}

struct DecodedState<'a> {
    level: f64,
    plugin_id: Option<&'a str>,
    plugin_state: &'a [u8],
}

fn encode_state(level: f64, plugin_id: &str, plugin_state: &[u8]) -> Vec<u8> {
    let plugin_id = plugin_id.as_bytes();
    let plugin_id_length = u16::try_from(plugin_id.len()).unwrap_or(u16::MAX);
    let plugin_id = &plugin_id[..plugin_id_length as usize];
    let mut bytes = Vec::with_capacity(26 + plugin_id.len() + plugin_state.len());
    bytes.extend_from_slice(STATE_MAGIC);
    bytes.extend_from_slice(&STATE_VERSION.to_le_bytes());
    bytes.extend_from_slice(&level.clamp(0.0, 1.0).to_le_bytes());
    bytes.extend_from_slice(&plugin_id_length.to_le_bytes());
    bytes.extend_from_slice(&(plugin_state.len() as u32).to_le_bytes());
    bytes.extend_from_slice(plugin_id);
    bytes.extend_from_slice(plugin_state);
    bytes
}

fn decode_state(bytes: &[u8]) -> Result<DecodedState<'_>, ()> {
    if bytes.len() < 24 || &bytes[..8] != STATE_MAGIC {
        return Err(());
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| ())?);
    let level = f64::from_le_bytes(bytes[12..20].try_into().map_err(|_| ())?);
    if !level.is_finite() {
        return Err(());
    }
    if version == 1 {
        let length = u32::from_le_bytes(bytes[20..24].try_into().map_err(|_| ())?) as usize;
        if bytes.len() != 24 + length {
            return Err(());
        }
        return Ok(DecodedState {
            level: level.clamp(0.0, 1.0),
            plugin_id: None,
            plugin_state: &bytes[24..],
        });
    }
    if version != STATE_VERSION || bytes.len() < 26 {
        return Err(());
    }
    let plugin_id_length = u16::from_le_bytes(bytes[20..22].try_into().map_err(|_| ())?) as usize;
    let state_length = u32::from_le_bytes(bytes[22..26].try_into().map_err(|_| ())?) as usize;
    let plugin_id_end = 26usize.checked_add(plugin_id_length).ok_or(())?;
    let state_end = plugin_id_end.checked_add(state_length).ok_or(())?;
    if bytes.len() != state_end {
        return Err(());
    }
    let plugin_id = std::str::from_utf8(&bytes[26..plugin_id_end]).map_err(|_| ())?;
    if plugin_id.is_empty() {
        return Err(());
    }
    Ok(DecodedState {
        level: level.clamp(0.0, 1.0),
        plugin_id: Some(plugin_id),
        plugin_state: &bytes[plugin_id_end..],
    })
}

/// The controller's own state: what the editor showed -- the instrument, the
/// program chosen in it and the parameter values -- beside the level.
///
/// It was the level alone, eight bytes. The processor's state carries the
/// instrument and its sound, and is the only half a host has to hand back,
/// so a project reopened in FL Studio played the instrument it was saved
/// with under an editor showing the first one, and the program chosen in the
/// editor was remembered nowhere at all. A host stores this beside the
/// processor's and returns it with `setState`; eight bytes are still read as
/// the level of a project saved before.
const CONTROLLER_STATE_MAGIC: &[u8; 8] = b"RFVSTUI1";

#[derive(Debug, PartialEq)]
struct ControllerState {
    level: f64,
    plugin_id: Option<String>,
    sound_id: Option<String>,
    values: Vec<(u32, f64)>,
}

fn encode_controller_state(
    level: f64,
    plugin_id: Option<&str>,
    sound_id: Option<&str>,
    values: &BTreeMap<u32, f64>,
) -> Vec<u8> {
    let text = |bytes: &mut Vec<u8>, value: Option<&str>| {
        let value = value.unwrap_or_default().as_bytes();
        let length = u16::try_from(value.len()).unwrap_or(u16::MAX);
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&value[..length as usize]);
    };
    let mut bytes = Vec::with_capacity(32 + values.len() * 12);
    bytes.extend_from_slice(CONTROLLER_STATE_MAGIC);
    bytes.extend_from_slice(&level.clamp(0.0, 1.0).to_le_bytes());
    text(&mut bytes, plugin_id);
    text(&mut bytes, sound_id);
    bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for (index, value) in values {
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn decode_controller_state(bytes: &[u8]) -> Result<ControllerState, ()> {
    if bytes.len() == 8 {
        let level = f64::from_le_bytes(bytes.try_into().map_err(|_| ())?);
        if !level.is_finite() {
            return Err(());
        }
        return Ok(ControllerState {
            level: level.clamp(0.0, 1.0),
            plugin_id: None,
            sound_id: None,
            values: Vec::new(),
        });
    }
    let mut at: usize = 0;
    let mut take = |length: usize| -> Result<&[u8], ()> {
        let end = at.checked_add(length).ok_or(())?;
        let slice = bytes.get(at..end).ok_or(())?;
        at = end;
        Ok(slice)
    };
    if take(8)? != CONTROLLER_STATE_MAGIC {
        return Err(());
    }
    let level = f64::from_le_bytes(take(8)?.try_into().map_err(|_| ())?);
    if !level.is_finite() {
        return Err(());
    }
    let mut text = || -> Result<Option<String>, ()> {
        let length = u16::from_le_bytes(take(2)?.try_into().map_err(|_| ())?) as usize;
        let value = std::str::from_utf8(take(length)?).map_err(|_| ())?;
        Ok((!value.is_empty()).then(|| value.to_owned()))
    };
    let plugin_id = text()?;
    let sound_id = text()?;
    let count = u32::from_le_bytes(take(4)?.try_into().map_err(|_| ())?) as usize;
    let mut values = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let index = u32::from_le_bytes(take(4)?.try_into().map_err(|_| ())?);
        let value = f64::from_le_bytes(take(8)?.try_into().map_err(|_| ())?);
        if !value.is_finite() {
            return Err(());
        }
        values.push((index, value));
    }
    if at != bytes.len() {
        return Err(());
    }
    Ok(ControllerState {
        level: level.clamp(0.0, 1.0),
        plugin_id,
        sound_id,
        values,
    })
}

unsafe fn read_stream(stream: *mut IBStream) -> Result<Vec<u8>, ()> {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else {
        return Err(());
    };
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let mut read = 0;
        let result =
            unsafe { stream.read(chunk.as_mut_ptr().cast(), chunk.len() as i32, &mut read) };
        if result != kResultOk && result != kResultTrue {
            return Err(());
        }
        if read <= 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
        if read < chunk.len() as i32 {
            break;
        }
    }
    Ok(bytes)
}

unsafe fn write_stream(stream: *mut IBStream, bytes: &[u8]) -> Result<(), ()> {
    let Some(stream) = (unsafe { ComRef::from_raw(stream) }) else {
        return Err(());
    };
    let mut offset = 0;
    while offset < bytes.len() {
        let amount = (bytes.len() - offset).min(i32::MAX as usize);
        let mut written = 0;
        let result = unsafe {
            stream.write(
                bytes[offset..].as_ptr().cast_mut().cast(),
                amount as i32,
                &mut written,
            )
        };
        if (result != kResultOk && result != kResultTrue) || written <= 0 {
            return Err(());
        }
        offset += written as usize;
    }
    Ok(())
}

fn copy_cstring(source: &str, destination: &mut [c_char]) {
    let source = CString::new(source).unwrap_or_default();
    destination.fill(0);
    for (source, destination) in source.as_bytes_with_nul().iter().zip(destination) {
        *destination = *source as c_char;
    }
}

fn copy_wstring(source: &str, destination: &mut [TChar]) {
    destination.fill(0);
    for (source, destination) in source.encode_utf16().zip(destination) {
        *destination = source;
    }
}

unsafe fn len_wstring(string: *const TChar) -> usize {
    let mut length = 0;
    while unsafe { *string.add(length) } != 0 {
        length += 1;
    }
    length
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn InitDll() -> bool {
    true
}

#[cfg(target_os = "windows")]
#[unsafe(no_mangle)]
extern "system" fn ExitDll() -> bool {
    // The host is about to unmap this DLL. Wasmtime's trap handlers are
    // process-global and live in our code, so they must come out first or
    // the host's next exception jumps into freed memory. See
    // `engine::unload_runtimes` and the runtime crate's
    // `unload_process_handlers` for the crash this closes.
    // Order matters: the WebView2 environment is a COM object registered
    // from this DLL and is released first, while every function it may call
    // back into is still mapped; Wasmtime's process-wide trap handlers come
    // out after it.
    diagnostic::write(format!(
        "ExitDll webview environment {}",
        webview_env::release()
    ));
    diagnostic::write(format!("ExitDll {:?}", engine::unload_runtimes()));
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn BundleEntry(_bundle: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "macos")]
#[unsafe(no_mangle)]
extern "system" fn BundleExit() -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleEntry(_module: *mut c_void) -> bool {
    true
}

#[cfg(target_os = "linux")]
#[unsafe(no_mangle)]
extern "system" fn ModuleExit() -> bool {
    true
}

#[unsafe(no_mangle)]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    diagnostic::write("GetPluginFactory");
    ComWrapper::new(Factory)
        .to_com_ptr::<IPluginFactory>()
        .unwrap()
        .into_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A logarithmic parameter reaches a host's automation lane the way it
    /// reaches a fader, and comes back the value it went out as.
    #[test]
    fn a_logarithmic_parameter_survives_the_host_round_trip() {
        let parameter = ParameterDescriptor {
            index: 0,
            id: "knob".into(),
            name: "Knob".into(),
            page: String::new(),
            group: None,
            order: 0,
            suggested_control: Default::default(),
            kind: ParameterKind::Float {
                minimum: 0.5,
                maximum: 128.0,
                default: 8.0,
                step: 0.08,
                unit: None,
                taper: ParameterTaper::Logarithmic,
            },
            flags: Default::default(),
        };
        // Half the host's fader is the value the control is centred on, not
        // the arithmetic middle of its range.
        let middle = parameter_normalized_to_plain(&parameter, 0.5);
        assert!(
            (middle - 8.0).abs() < 1e-9,
            "half the lane reached {middle}"
        );
        for value in [0.5, 1.0, 8.0, 42.0, 128.0] {
            let normalized = parameter_plain_to_normalized(&parameter, value).unwrap();
            assert!((0.0..=1.0).contains(&normalized), "{value} left the lane");
            let back = parameter_normalized_to_plain(&parameter, normalized);
            assert!(
                (back - value).abs() <= 1e-9 * value.abs().max(1.0),
                "{value} came back as {back}"
            );
        }
    }

    #[test]
    fn state_round_trip_preserves_plugin_blob_and_level() {
        let state = encode_state(0.625, "org.rackforge.rf-106", &[1, 2, 3, 4]);
        let decoded = decode_state(&state).unwrap();
        assert_eq!(decoded.level, 0.625);
        assert_eq!(decoded.plugin_id, Some("org.rackforge.rf-106"));
        assert_eq!(decoded.plugin_state, &[1, 2, 3, 4]);
    }

    #[test]
    fn state_rejects_truncation_and_unknown_versions() {
        let mut state = encode_state(1.0, "org.rackforge.test", &[1]);
        assert!(decode_state(&state[..state.len() - 1]).is_err());
        state[8] = 3;
        assert!(decode_state(&state).is_err());
    }

    #[test]
    fn state_decoder_keeps_version_one_projects_compatible() {
        let mut state = Vec::new();
        state.extend_from_slice(STATE_MAGIC);
        state.extend_from_slice(&1_u32.to_le_bytes());
        state.extend_from_slice(&0.75_f64.to_le_bytes());
        state.extend_from_slice(&3_u32.to_le_bytes());
        state.extend_from_slice(&[7, 8, 9]);
        let decoded = decode_state(&state).unwrap();
        assert_eq!(decoded.level, 0.75);
        assert_eq!(decoded.plugin_id, None);
        assert_eq!(decoded.plugin_state, &[7, 8, 9]);
    }

    /// The editor's state comes back as it went out: the instrument, the
    /// program chosen in it and the values it showed.
    #[test]
    fn controller_state_round_trips_what_the_editor_showed() {
        let values = BTreeMap::from([(0, 0.25), (3, -12.0), (40, 1.0)]);
        let bytes = encode_controller_state(
            0.5,
            Some("org.rackforge.musette"),
            Some("musette-paris"),
            &values,
        );
        assert_eq!(
            decode_controller_state(&bytes),
            Ok(ControllerState {
                level: 0.5,
                plugin_id: Some("org.rackforge.musette".into()),
                sound_id: Some("musette-paris".into()),
                values: values.into_iter().collect(),
            })
        );
        let empty = encode_controller_state(1.0, None, None, &BTreeMap::new());
        let decoded = decode_controller_state(&empty).unwrap();
        assert_eq!((decoded.plugin_id, decoded.sound_id), (None, None));
    }

    /// A project saved before the controller kept anything but its level.
    #[test]
    fn controller_state_reads_the_old_eight_bytes_as_the_level() {
        let decoded = decode_controller_state(&0.75_f64.to_le_bytes()).unwrap();
        assert_eq!(decoded.level, 0.75);
        assert_eq!(decoded.plugin_id, None);
        assert!(decoded.values.is_empty());
    }

    #[test]
    fn controller_state_refuses_what_it_did_not_write() {
        let bytes = encode_controller_state(
            1.0,
            Some("org.rackforge.rf-106"),
            Some("pad"),
            &BTreeMap::from([(1, 0.5)]),
        );
        assert!(decode_controller_state(&bytes[..bytes.len() - 1]).is_err());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(decode_controller_state(&longer).is_err());
        let mut foreign = bytes;
        foreign[0] = b'X';
        assert!(decode_controller_state(&foreign).is_err());
    }

    /// Restoring the instrument already shown keeps the program and values
    /// already restored: a host may return the controller's own state before
    /// the processor's, which names the same instrument.
    #[test]
    fn restoring_the_shown_instrument_keeps_the_restored_program_and_values() {
        let controller = RackForgeController::new();
        let shared = &controller.shared;
        let Some(model) = shared.catalog.last().cloned() else {
            return;
        };
        let Some(sound_id) = model.preset_names.keys().last().cloned() else {
            return;
        };
        shared.restore_plugin(&model.plugin_id).unwrap();
        shared.restore_editor(Some(sound_id.clone()), &[(0, 0.123)]);
        let revision = shared.revision.load(Ordering::Relaxed);

        shared.restore_plugin(&model.plugin_id).unwrap();

        assert_eq!(
            shared.selected_sound_id.read().unwrap().as_deref(),
            Some(sound_id.as_str())
        );
        assert_eq!(shared.plugin_value(0), Some(0.123));
        assert_eq!(shared.revision.load(Ordering::Relaxed), revision);
    }

    #[test]
    fn midi_conversion_clamps_channel_note_and_velocity() {
        let event = midi_event(7, 0x90, 20, 200, 2.0);
        assert!(event.origin_7bit);
        assert_eq!(event.to_midi1().data, [0x9f, 127, 127]);
    }

    /// A value on the `k / 127` grid is the byte it came from, lifted and
    /// flagged; a value between grid points keeps its width unflagged.
    #[test]
    fn a_seven_bit_source_stays_bytes_and_a_finer_one_keeps_its_width() {
        let byte = midi_event(0, 0x90, 0, 60, 100.0 / 127.0);
        assert!(byte.origin_7bit);
        assert_eq!(byte.to_midi1().data, [0x90, 60, 100]);

        let fine = midi_event(0, 0x90, 0, 60, 0.5);
        assert!(!fine.origin_7bit);
        assert_eq!(
            fine.message,
            Midi2Message::NoteOn {
                note: 60,
                velocity: 32768
            }
        );

        let lifted = midi_event(0, 0x80, 0, 60, 0.3);
        assert_eq!(
            lifted.message,
            Midi2Message::NoteOff {
                note: 60,
                velocity: Some(19661)
            }
        );

        let pedal = controller_event(1, 0, 64, 0.5);
        assert!(!pedal.origin_7bit);
        assert_eq!(
            pedal.message,
            Midi2Message::ControlChange {
                controller: 64,
                value: 2_147_483_648
            }
        );
        let pedal_byte = controller_event(1, 0, 64, 1.0);
        assert!(pedal_byte.origin_7bit);
        assert_eq!(pedal_byte.to_midi1().data, [0xB0, 64, 127]);
        // Pitch bend keeps its 14-bit byte form whatever the value.
        assert!(controller_event(1, 0, 129, 0.3).origin_7bit);
    }

    #[test]
    fn midi_mapping_ids_round_trip_for_every_channel() {
        for channel in 0..16 {
            for controller in 0..130 {
                let id = midi_parameter_id(channel, controller);
                assert_eq!(decode_midi_parameter(id), Some((channel, controller)));
            }
        }
        assert_eq!(decode_midi_parameter(MASTER_LEVEL), None);
    }

    #[test]
    fn controller_parameters_preserve_sustain_and_pitch_bend_resolution() {
        let sustain = controller_midi_event(12, 2, 64, 1.0);
        assert_eq!(sustain.frame, 12);
        assert_eq!(sustain.data, [0xB2, 64, 127]);

        let centered = controller_midi_event(3, 0, 129, 0.5);
        assert_eq!(centered.data, [0xE0, 0, 64]);
        let maximum = controller_midi_event(3, 15, 129, 1.0);
        assert_eq!(maximum.data, [0xEF, 127, 127]);
    }

    #[test]
    fn controller_exposes_an_editor_view() {
        let controller = RackForgeController::new();
        let name = CString::new("editor").unwrap();
        let raw = unsafe { controller.createView(name.as_ptr()) };
        assert!(!raw.is_null());
        let view = unsafe { ComPtr::<IPlugView>::from_raw(raw) }.unwrap();
        let platform = CString::new("HWND").unwrap();
        #[cfg(windows)]
        assert_eq!(
            unsafe { view.isPlatformTypeSupported(platform.as_ptr()) },
            kResultTrue
        );
        #[cfg(not(windows))]
        assert_eq!(
            unsafe { view.isPlatformTypeSupported(platform.as_ptr()) },
            kResultFalse
        );
    }
}
