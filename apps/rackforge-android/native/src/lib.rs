use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jstring};
use keylab_essential_mk3::protocol as keylab_protocol;
use rackforge_control_api::ControlResponse;
use rackforge_core::{
    LoadedPlugin, PluginInstance, PluginPackage, PluginStateStore, PluginStorage,
    midi_hotplug::{PanicScope, panic_packets},
    plugin_parameters, set_plugin_parameter,
};
use rackforge_plugin_api::{
    PROGRAM_EDITOR_SCHEMA_VERSION, PluginKind, PreparedProgram, PresetCatalog, ProgramDocument,
    ProgramEditRequest, ProgramEditorValue, ProgramEditorView, ProgramFieldEditRequest,
    WebSurfaceKind, abi::MidiEventV1,
};
use rackforge_repository::{
    PluginUserDataRemovalOptions, cleanup_uninstall_tombstones, inspect_local_archive,
    install_local_archive, remove_plugin_user_data, uninstall_plugin,
};
use rackforge_session_api::{
    HostControlTarget, InstanceId, MasterLevel, MasterPan, ProgramDraftState,
};
use rackforge_surface_runtime::{
    ActiveMode, Input as SurfaceInput, Menu as SurfaceMenu, MenuCommand, PlayPlugin, PlaySound,
};
use std::cell::UnsafeCell;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{CStr, c_char, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SAMPLE_RATE: f64 = 48_000.0;
const MAX_FRAMES: u32 = 4_096;
const MAX_PENDING_MIDI_EVENTS: usize = 256;
const AUDIO_RENDER_QUEUE_CAPACITY_FRAMES: usize = 2_048;
const LOW_RENDER_BLOCK_FRAMES: usize = 192;
const BALANCED_RENDER_BLOCK_FRAMES: usize = 384;
const LOW_RENDER_AHEAD_FRAMES: usize = 384;
const BALANCED_RENDER_AHEAD_FRAMES: usize = 1_152;

static ENGINE: OnceLock<Mutex<Option<AndroidEngine>>> = OnceLock::new();
static AUDIO: OnceLock<Mutex<Option<NativeAudioOutput>>> = OnceLock::new();
static MIDI_QUEUE: OnceLock<Mutex<VecDeque<MidiEventV1>>> = OnceLock::new();
static CONTROLLER_MENU: OnceLock<Mutex<AndroidControllerMenu>> = OnceLock::new();
static OUTPUT_GAIN_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_LEVEL_TARGET_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_LEVEL_CURRENT_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_PAN_LEFT_TARGET_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_PAN_LEFT_CURRENT_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_PAN_RIGHT_TARGET_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_PAN_RIGHT_CURRENT_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static AUDIO_ERROR: AtomicI32 = AtomicI32::new(AAUDIO_OK);
static MIDI_DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);
static MIDI_PANIC_COUNT: AtomicU64 = AtomicU64::new(0);
static AUDIO_CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);
static AUDIO_CALLBACK_FRAMES: AtomicU64 = AtomicU64::new(0);
static AUDIO_CALLBACK_TOTAL_NANOS: AtomicU64 = AtomicU64::new(0);
static AUDIO_CALLBACK_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
static AUDIO_ENGINE_LOCK_MISSES: AtomicU64 = AtomicU64::new(0);
static AUDIO_RENDER_ERRORS: AtomicU64 = AtomicU64::new(0);
static AUDIO_NONFINITE_SAMPLES: AtomicU64 = AtomicU64::new(0);
static AUDIO_CLIPPED_SAMPLES: AtomicU64 = AtomicU64::new(0);
static AUDIO_RENDER_QUEUE_UNDERRUNS: AtomicU64 = AtomicU64::new(0);
static AUDIO_RENDER_QUEUE_UNDERRUN_FRAMES: AtomicU64 = AtomicU64::new(0);
static AUDIO_RENDER_THREAD_PRIORITY_RESULT: AtomicI32 = AtomicI32::new(0);
static AUDIO_RECOVERY_RAMP_PENDING: AtomicU32 = AtomicU32::new(0);
static AUDIO_LAST_LEFT_BITS: AtomicU32 = AtomicU32::new(0.0_f32.to_bits());
static AUDIO_LAST_RIGHT_BITS: AtomicU32 = AtomicU32::new(0.0_f32.to_bits());

const AAUDIO_OK: i32 = 0;
const AAUDIO_DIRECTION_OUTPUT: i32 = 0;
const AAUDIO_SHARING_MODE_EXCLUSIVE: i32 = 0;
const AAUDIO_SHARING_MODE_SHARED: i32 = 1;
const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
const AAUDIO_PERFORMANCE_MODE_NONE: i32 = 10;
const AAUDIO_PERFORMANCE_MODE_LOW_LATENCY: i32 = 12;
const AAUDIO_CALLBACK_RESULT_CONTINUE: i32 = 0;
const CONTROLLER_LONG_PRESS_MS: u128 = 700;
const CONTROLLER_HOME_CHORD_MS: u128 = 250;
const MASTER_SMOOTHING_FACTOR: f32 = 0.02;
const DROPOUT_FADE_FRAMES: usize = 64;
const HOST_CONTROL_HEADER_MS: u64 = 1_500;
const PRIO_PROCESS: i32 = 0;
const ANDROID_AUDIO_THREAD_NICE: i32 = -16;
const ANDROID_INSTANCE_ID: &str = "android-main";

struct AndroidControllerMenu {
    menu: SurfaceMenu,
    button_down: [Option<Instant>; 4],
    button_long_fired: [bool; 4],
    installed_plugins: Vec<PlayPlugin>,
    plugins: BTreeMap<String, ControllerPluginInfo>,
}

#[derive(Clone)]
struct ControllerPluginInfo {
    root: String,
    name: String,
    version: String,
}

impl Default for AndroidControllerMenu {
    fn default() -> Self {
        Self {
            menu: SurfaceMenu::default(),
            button_down: [None; 4],
            button_long_fired: [false; 4],
            installed_plugins: Vec::new(),
            plugins: BTreeMap::new(),
        }
    }
}

impl AndroidControllerMenu {
    fn render_response(&self, command: Option<serde_json::Value>) -> Result<String> {
        Ok(serde_json::json!({
            "plan": controller_plan_value(keylab_protocol::render_messages(&self.menu.render()))?,
            "command": command,
        })
        .to_string())
    }

    fn render_host_control(&self, target: HostControlTarget, value: u8) -> Result<String> {
        let header = keylab_protocol::host_control_header(target, value);
        Ok(serde_json::json!({
            "plan": controller_plan_value(keylab_protocol::transient_header_messages(&header))?,
            "command": null,
            "restore_header_after_ms": HOST_CONTROL_HEADER_MS,
        })
        .to_string())
    }

    fn apply(&mut self, input: SurfaceInput) -> Option<serde_json::Value> {
        self.menu.apply_input(input);
        let command = self.menu.take_command();
        if let Some(MenuCommand::ReturnToActiveMode {
            mode,
            selected_sound_id,
            ..
        }) = command.as_ref()
        {
            self.menu
                .complete_return_to_active_mode(*mode, selected_sound_id.as_deref());
        }
        if matches!(command.as_ref(), Some(MenuCommand::ForceHome)) {
            self.menu
                .set_play_plugins(self.installed_plugins.clone(), None);
        }
        let command = command.and_then(|command| self.command_json(command));
        while self.menu.take_command().is_some() {}
        command
    }

    fn command_json(&self, command: MenuCommand) -> Option<serde_json::Value> {
        match command {
            MenuCommand::SetActiveMode { mode } => Some(serde_json::json!({
                "type": "set_mode",
                "mode": active_mode_name(mode),
            })),
            MenuCommand::SelectPlugin { instance_id } => {
                let plugin = self.plugins.get(&instance_id)?;
                Some(serde_json::json!({
                    "type": "select_plugin",
                    "root": plugin.root,
                    "name": plugin.name,
                    "version": plugin.version,
                }))
            }
            MenuCommand::SelectSound { id } => Some(serde_json::json!({
                "type": "select_sound",
                "sound_id": id,
            })),
            MenuCommand::ReturnToActiveMode {
                mode,
                selected_sound_id,
                ..
            } => Some(serde_json::json!({
                "type": "return_mode",
                "mode": active_mode_name(mode),
                "sound_id": selected_sound_id,
            })),
            MenuCommand::ForceHome => Some(serde_json::json!({ "type": "force_home" })),
            _ => None,
        }
    }

    fn handle_surface(
        &mut self,
        input: SurfaceInput,
        phase: keylab_protocol::InputPhase,
    ) -> Result<String> {
        use keylab_protocol::InputPhase;
        let mut command = None;

        if let Some(index) = surface_button_index(input) {
            match phase {
                InputPhase::Press => {
                    self.button_down[index] = Some(Instant::now());
                    self.button_long_fired[index] = false;
                    self.menu.set_button_pressed(input, true);
                }
                InputPhase::Release => {
                    let now = Instant::now();
                    let long_fired = std::mem::replace(&mut self.button_long_fired[index], false);
                    let home_chord = !long_fired
                        && matches!(index, 0 | 3)
                        && self.button_down[0].is_some()
                        && self.button_down[3].is_some()
                        && controller_home_chord_ready(
                            self.button_down[0],
                            self.button_down[3],
                            now,
                        );
                    if home_chord {
                        self.button_down[0] = None;
                        self.button_down[3] = None;
                        self.button_long_fired[0] = false;
                        self.button_long_fired[3] = false;
                        self.menu.set_button_pressed(SurfaceInput::Button1, false);
                        self.menu.set_button_pressed(SurfaceInput::Button4, false);
                        command = self.apply(SurfaceInput::HomeChord);
                    } else {
                        let started = self.button_down[index].take();
                        self.menu.set_button_pressed(input, false);
                        if !long_fired && let Some(started) = started {
                            command = self.apply(
                                if now.duration_since(started).as_millis()
                                    >= CONTROLLER_LONG_PRESS_MS
                                {
                                    surface_long_input(index)
                                } else {
                                    input
                                },
                            );
                        }
                    }
                }
                InputPhase::Turn => {}
            }
            return self.render_response(command);
        }

        match (input, phase) {
            (SurfaceInput::EncoderLeft | SurfaceInput::EncoderRight, InputPhase::Turn)
            | (SurfaceInput::EncoderPress, InputPhase::Release)
            | (SurfaceInput::KeyboardParts, InputPhase::Press) => command = self.apply(input),
            _ => {}
        }
        self.render_response(command)
    }

    fn poll_long_press(&mut self, now: Instant) -> Result<Option<String>> {
        if let (Some(first), Some(fourth)) = (self.button_down[0], self.button_down[3]) {
            let separation = if first >= fourth {
                first.duration_since(fourth)
            } else {
                fourth.duration_since(first)
            };
            if separation.as_millis() <= CONTROLLER_HOME_CHORD_MS {
                if now.duration_since(first.max(fourth)).as_millis() < CONTROLLER_LONG_PRESS_MS {
                    return Ok(None);
                }
                self.button_long_fired[0] = true;
                self.button_long_fired[3] = true;
                self.menu.set_button_pressed(SurfaceInput::Button1, false);
                self.menu.set_button_pressed(SurfaceInput::Button4, false);
                let command = self.apply(SurfaceInput::HomeChord);
                return self.render_response(command).map(Some);
            }
        }

        let Some(index) = self
            .button_down
            .iter()
            .enumerate()
            .find_map(|(index, started)| {
                (!self.button_long_fired[index]
                    && started.is_some_and(|started| {
                        now.duration_since(started).as_millis() >= CONTROLLER_LONG_PRESS_MS
                    }))
                .then_some(index)
            })
        else {
            return Ok(None);
        };
        self.button_long_fired[index] = true;
        self.menu
            .set_button_pressed(surface_short_input(index), false);
        let command = self.apply(surface_long_input(index));
        self.render_response(command).map(Some)
    }
}

fn active_mode_name(mode: ActiveMode) -> &'static str {
    match mode {
        ActiveMode::Idle => "idle",
        ActiveMode::Live => "live",
        ActiveMode::Play => "play",
    }
}

fn surface_button_index(input: SurfaceInput) -> Option<usize> {
    match input {
        SurfaceInput::Button1 => Some(0),
        SurfaceInput::Button2 => Some(1),
        SurfaceInput::Button3 => Some(2),
        SurfaceInput::Button4 => Some(3),
        _ => None,
    }
}

fn surface_long_input(index: usize) -> SurfaceInput {
    [
        SurfaceInput::Button1Long,
        SurfaceInput::Button2Long,
        SurfaceInput::Button3Long,
        SurfaceInput::Button4Long,
    ][index]
}

fn surface_short_input(index: usize) -> SurfaceInput {
    [
        SurfaceInput::Button1,
        SurfaceInput::Button2,
        SurfaceInput::Button3,
        SurfaceInput::Button4,
    ][index]
}

fn controller_home_chord_ready(
    first: Option<Instant>,
    fourth: Option<Instant>,
    now: Instant,
) -> bool {
    let (Some(first), Some(fourth)) = (first, fourth) else {
        return false;
    };
    let separation = if first >= fourth {
        first.duration_since(fourth)
    } else {
        fourth.duration_since(first)
    };
    separation.as_millis() <= CONTROLLER_HOME_CHORD_MS
        && now.duration_since(first.max(fourth)).as_millis() >= CONTROLLER_LONG_PRESS_MS
}

#[repr(C)]
struct AAudioStreamBuilder {
    _private: [u8; 0],
}

#[repr(C)]
struct AAudioStream {
    _private: [u8; 0],
}

type AAudioDataCallback = unsafe extern "C" fn(
    stream: *mut AAudioStream,
    user_data: *mut c_void,
    audio_data: *mut c_void,
    num_frames: i32,
) -> i32;

type AAudioErrorCallback =
    unsafe extern "C" fn(stream: *mut AAudioStream, user_data: *mut c_void, error: i32);

unsafe extern "C" {
    fn setpriority(which: i32, who: u32, priority: i32) -> i32;
}

#[link(name = "aaudio")]
unsafe extern "C" {
    fn AAudio_createStreamBuilder(builder: *mut *mut AAudioStreamBuilder) -> i32;
    fn AAudioStreamBuilder_delete(builder: *mut AAudioStreamBuilder) -> i32;
    fn AAudioStreamBuilder_setDeviceId(builder: *mut AAudioStreamBuilder, device_id: i32);
    fn AAudioStreamBuilder_setDirection(builder: *mut AAudioStreamBuilder, direction: i32);
    fn AAudioStreamBuilder_setSharingMode(builder: *mut AAudioStreamBuilder, sharing_mode: i32);
    fn AAudioStreamBuilder_setPerformanceMode(builder: *mut AAudioStreamBuilder, mode: i32);
    fn AAudioStreamBuilder_setFormat(builder: *mut AAudioStreamBuilder, format: i32);
    fn AAudioStreamBuilder_setChannelCount(builder: *mut AAudioStreamBuilder, channel_count: i32);
    fn AAudioStreamBuilder_setSampleRate(builder: *mut AAudioStreamBuilder, sample_rate: i32);
    fn AAudioStreamBuilder_setDataCallback(
        builder: *mut AAudioStreamBuilder,
        callback: Option<AAudioDataCallback>,
        user_data: *mut c_void,
    );
    fn AAudioStreamBuilder_setFramesPerDataCallback(
        builder: *mut AAudioStreamBuilder,
        num_frames: i32,
    );
    fn AAudioStreamBuilder_setErrorCallback(
        builder: *mut AAudioStreamBuilder,
        callback: Option<AAudioErrorCallback>,
        user_data: *mut c_void,
    );
    fn AAudioStreamBuilder_openStream(
        builder: *mut AAudioStreamBuilder,
        stream: *mut *mut AAudioStream,
    ) -> i32;
    fn AAudioStream_requestStart(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_requestStop(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_close(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getFramesPerBurst(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getFramesPerDataCallback(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_setBufferSizeInFrames(stream: *mut AAudioStream, frames: i32) -> i32;
    fn AAudioStream_getBufferSizeInFrames(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getBufferCapacityInFrames(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getXRunCount(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getSampleRate(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getSharingMode(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_getPerformanceMode(stream: *mut AAudioStream) -> i32;
    fn AAudio_convertResultToText(result: i32) -> *const c_char;
}

struct AndroidEngine {
    instance: SendablePluginInstance,
    runtime: SendableLoadedPlugin,
    midi: Vec<MidiEventV1>,
    plugin_id: String,
    plugin_name: String,
    plugin_version: String,
    package_root: PathBuf,
    web_entry: String,
    config_web_entry: Option<String>,
    resource_requirements: Vec<rackforge_plugin_api::ResourceRequirement>,
    resource_overrides: BTreeMap<String, PathBuf>,
    data_root: PathBuf,
    catalog: PresetCatalog,
    selected_sound_id: String,
    program_draft: Option<ProgramDraftState>,
    program_previous_sound_id: Option<String>,
    next_program_draft_id: u64,
}

struct SendablePluginInstance(PluginInstance<'static>);

#[derive(Clone, Copy)]
struct SendableLoadedPlugin(&'static LoadedPlugin);

// SAFETY: access to the plugin instance is serialized by ENGINE's mutex. The
// JNI bridge never exposes the instance pointer to Java or another callback.
unsafe impl Send for SendablePluginInstance {}

// SAFETY: AndroidEngine accepts only validated portable wasm-v1 packages.
// Their LoadedPlugin backend is immutable compiled Wasm state; native plugin
// host pointers can never inhabit this wrapper on Android.
unsafe impl Send for SendableLoadedPlugin {}
unsafe impl Sync for SendableLoadedPlugin {}

struct AudioRenderQueue {
    samples: Box<[UnsafeCell<f32>]>,
    capacity_frames: usize,
    read_frame: AtomicUsize,
    write_frame: AtomicUsize,
}

// SAFETY: AudioRenderQueue has exactly one producer (the render worker) and
// one consumer (AAudio's data callback). The producer publishes complete
// frames with a release store before the consumer reads them, and the
// consumer publishes released slots before the producer reuses them.
unsafe impl Send for AudioRenderQueue {}
unsafe impl Sync for AudioRenderQueue {}

impl AudioRenderQueue {
    fn new(capacity_frames: usize) -> Self {
        let samples = (0..capacity_frames * 2)
            .map(|_| UnsafeCell::new(0.0_f32))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            samples,
            capacity_frames,
            read_frame: AtomicUsize::new(0),
            write_frame: AtomicUsize::new(0),
        }
    }

    fn queued_frames(&self) -> usize {
        self.write_frame
            .load(Ordering::Acquire)
            .saturating_sub(self.read_frame.load(Ordering::Acquire))
    }

    fn push(&self, input: &[f32]) -> bool {
        if !input.len().is_multiple_of(2) {
            return false;
        }
        let frames = input.len() / 2;
        let write = self.write_frame.load(Ordering::Relaxed);
        let read = self.read_frame.load(Ordering::Acquire);
        if frames
            > self
                .capacity_frames
                .saturating_sub(write.saturating_sub(read))
        {
            return false;
        }
        for (sample_index, sample) in input.iter().copied().enumerate() {
            let frame = write + sample_index / 2;
            let channel = sample_index % 2;
            let slot = (frame % self.capacity_frames) * 2 + channel;
            // SAFETY: only the producer writes unpublished ring slots.
            unsafe { *self.samples[slot].get() = sample };
        }
        self.write_frame.store(write + frames, Ordering::Release);
        true
    }

    fn pop(&self, output: &mut [f32]) -> usize {
        debug_assert!(output.len().is_multiple_of(2));
        let requested_frames = output.len() / 2;
        let read = self.read_frame.load(Ordering::Relaxed);
        let write = self.write_frame.load(Ordering::Acquire);
        let frames = requested_frames.min(write.saturating_sub(read));
        for (sample_index, output_sample) in output[..frames * 2].iter_mut().enumerate() {
            let frame = read + sample_index / 2;
            let channel = sample_index % 2;
            let slot = (frame % self.capacity_frames) * 2 + channel;
            // SAFETY: the producer published these slots before advancing
            // write_frame and cannot reuse them until read_frame advances.
            *output_sample = unsafe { *self.samples[slot].get() };
        }
        self.read_frame.store(read + frames, Ordering::Release);
        frames
    }
}

struct AudioRenderWorker {
    queue: Arc<AudioRenderQueue>,
    stop: Arc<AtomicBool>,
    thread: thread::Thread,
    handle: Option<JoinHandle<()>>,
}

impl AudioRenderWorker {
    fn start(block_frames: usize, render_ahead_frames: usize) -> Result<Self> {
        let queue = Arc::new(AudioRenderQueue::new(AUDIO_RENDER_QUEUE_CAPACITY_FRAMES));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_queue = Arc::clone(&queue);
        let worker_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("RF-AudioRender".to_owned())
            .spawn(move || {
                // Android reserves this nice level for time-sensitive audio
                // work. The render-ahead queue remains the correctness
                // mechanism if a vendor kernel declines the priority request.
                let priority_result =
                    unsafe { setpriority(PRIO_PROCESS, 0, ANDROID_AUDIO_THREAD_NICE) };
                AUDIO_RENDER_THREAD_PRIORITY_RESULT.store(priority_result, Ordering::Relaxed);
                let mut scratch = vec![0.0_f32; block_frames * 2];
                while !worker_stop.load(Ordering::Acquire) {
                    if worker_queue.queued_frames() + block_frames > render_ahead_frames {
                        thread::park_timeout(Duration::from_millis(1));
                        continue;
                    }
                    scratch.fill(0.0);
                    let rendered = match engine().lock() {
                        Ok(mut guard) => match guard.as_mut() {
                            Some(engine) => {
                                engine.render(block_frames as u32, &mut scratch).is_ok()
                            }
                            None => false,
                        },
                        Err(_) => false,
                    };
                    if !rendered {
                        AUDIO_RENDER_ERRORS.fetch_add(1, Ordering::Relaxed);
                        scratch.fill(0.0);
                    }
                    if !worker_queue.push(&scratch) {
                        thread::yield_now();
                    }
                }
            })
            .context("starting Android audio render worker")?;
        let worker_thread = handle.thread().clone();
        let worker = Self {
            queue,
            stop,
            thread: worker_thread,
            handle: Some(handle),
        };
        let deadline = Instant::now() + Duration::from_millis(250);
        while worker.queue.queued_frames() < render_ahead_frames && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        Ok(worker)
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread.unpark();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for AudioRenderWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

struct AudioCallbackContext {
    queue: Arc<AudioRenderQueue>,
    render_thread: thread::Thread,
}

struct NativeAudioOutput {
    stream: *mut AAudioStream,
    callback_context: Box<AudioCallbackContext>,
    renderer: AudioRenderWorker,
}

// SAFETY: the stream is controlled through AAudio's thread-safe lifecycle API
// and is only replaced or dropped while held by AUDIO's mutex.
unsafe impl Send for NativeAudioOutput {}

impl NativeAudioOutput {
    fn open(device_id: i32, latency_mode: i32) -> Result<Self> {
        let (performance_mode, buffer_bursts, callback_frames, render_block, render_ahead) =
            match latency_mode {
                0 => (
                    AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
                    2,
                    0,
                    LOW_RENDER_BLOCK_FRAMES,
                    LOW_RENDER_AHEAD_FRAMES,
                ),
                1 => (
                    AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
                    3,
                    0,
                    BALANCED_RENDER_BLOCK_FRAMES,
                    BALANCED_RENDER_AHEAD_FRAMES,
                ),
                2 => (
                    AAUDIO_PERFORMANCE_MODE_NONE,
                    4,
                    0,
                    BALANCED_RENDER_BLOCK_FRAMES,
                    BALANCED_RENDER_AHEAD_FRAMES,
                ),
                _ => bail!("invalid Android latency mode {latency_mode}"),
            };
        let renderer = AudioRenderWorker::start(render_block, render_ahead)?;
        let mut callback_context = Box::new(AudioCallbackContext {
            queue: Arc::clone(&renderer.queue),
            render_thread: renderer.thread.clone(),
        });
        let user_data = (&mut *callback_context as *mut AudioCallbackContext).cast::<c_void>();
        let stream = if latency_mode == 0 {
            match open_aaudio_stream(
                device_id,
                AAUDIO_SHARING_MODE_EXCLUSIVE,
                performance_mode,
                buffer_bursts,
                callback_frames,
                user_data,
            ) {
                Ok(stream) => stream,
                Err(exclusive_error) => open_aaudio_stream(
                    device_id,
                    AAUDIO_SHARING_MODE_SHARED,
                    performance_mode,
                    buffer_bursts,
                    callback_frames,
                    user_data,
                )
                .with_context(|| {
                    format!("exclusive AAudio open also failed: {exclusive_error:#}")
                })?,
            }
        } else {
            open_aaudio_stream(
                device_id,
                AAUDIO_SHARING_MODE_SHARED,
                performance_mode,
                buffer_bursts,
                callback_frames,
                user_data,
            )?
        };
        Ok(Self {
            stream,
            callback_context,
            renderer,
        })
    }

    fn grow_buffer(&self) -> bool {
        if self.stream.is_null() {
            return false;
        }
        // SAFETY: self owns a live stream and AAudio permits changing the
        // application buffer size while the stream is running.
        unsafe {
            let burst = AAudioStream_getFramesPerBurst(self.stream);
            let current = AAudioStream_getBufferSizeInFrames(self.stream);
            let capacity = AAudioStream_getBufferCapacityInFrames(self.stream);
            if burst <= 0 || current < 0 || capacity <= current {
                return false;
            }
            let requested = current.saturating_add(burst).min(capacity);
            AAudioStream_setBufferSizeInFrames(self.stream, requested) > current
        }
    }
}

impl Drop for NativeAudioOutput {
    fn drop(&mut self) {
        if self.stream.is_null() {
            return;
        }
        // SAFETY: stream was returned by AAudio and this is its sole owner.
        unsafe {
            let _ = AAudioStream_requestStop(self.stream);
            let _ = AAudioStream_close(self.stream);
        }
        self.stream = ptr::null_mut();
        self.renderer.stop();
    }
}

impl AndroidEngine {
    fn open_package(package_root: &Path, data_root: PathBuf) -> Result<Self> {
        let package = PluginPackage::open(package_root)
            .with_context(|| format!("opening {}", package_root.display()))?;
        let manifest = package.manifest();
        if manifest.kind != PluginKind::Instrument {
            bail!("Android PLAY currently supports instrument plugins only");
        }
        manifest
            .portable_component()
            .context("Android requires a portable wasm-v1 plugin")?;
        let plugin_id = manifest.id.clone();
        let plugin_name = manifest.name.clone();
        let plugin_version = manifest.version.clone();
        let web_entry = manifest
            .web_ui
            .as_ref()
            .and_then(|web| {
                web.surfaces
                    .iter()
                    .find(|surface| surface.kind == WebSurfaceKind::Play)
            })
            .map(|surface| surface.entry.clone())
            .context("plugin does not expose a PLAY Web surface")?;
        let config_web_entry = manifest.web_ui.as_ref().and_then(|web| {
            web.surfaces
                .iter()
                .find(|surface| surface.kind == WebSurfaceKind::Config)
                .map(|surface| surface.entry.clone())
        });
        let resource_requirements = manifest.resources.clone();
        let package_root = package.root().to_path_buf();
        // SAFETY: Android installs only validated, sandboxed wasm-v1 packages.
        let plugin =
            unsafe { LoadedPlugin::load(&package, None, &BTreeMap::new(), Some(&data_root)) }
                .context("loading the portable plugin runtime")?;
        let plugin: &'static LoadedPlugin = Box::leak(Box::new(plugin));
        let mut instance = plugin.create_instance()?;
        let catalog = instance.preset_catalog()?;
        let selected_sound_id = catalog
            .presets
            .first()
            .or_else(|| plugin.presets().presets.first())
            .context("plugin exposes no playable preset")?
            .id
            .clone();
        instance
            .load_preset(&selected_sound_id)
            .with_context(|| format!("loading preset {selected_sound_id:?}"))?;
        instance.activate(SAMPLE_RATE, MAX_FRAMES, 0, 2)?;
        Ok(Self {
            instance: SendablePluginInstance(instance),
            runtime: SendableLoadedPlugin(plugin),
            midi: Vec::with_capacity(256),
            plugin_id,
            plugin_name,
            plugin_version,
            package_root,
            web_entry,
            config_web_entry,
            resource_requirements,
            resource_overrides: BTreeMap::new(),
            data_root,
            catalog,
            selected_sound_id,
            program_draft: None,
            program_previous_sound_id: None,
            next_program_draft_id: 1,
        })
    }

    fn select_sound(&mut self, sound_id: &str) -> Result<()> {
        if self.program_draft.is_some() {
            bail!("finish or cancel the active program edit before selecting another sound");
        }
        if !self
            .catalog
            .presets
            .iter()
            .any(|preset| preset.id == sound_id)
        {
            bail!("plugin does not expose sound {sound_id:?}");
        }
        self.instance.0.load_preset(sound_id)?;
        self.selected_sound_id = sound_id.to_owned();
        Ok(())
    }

    fn plugin_parameter_command(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let instance_id = params
            .get("instance_id")
            .and_then(serde_json::Value::as_str)
            .context("plugin parameter command is missing instance_id")?;
        if instance_id != ANDROID_INSTANCE_ID {
            bail!("plugin instance {instance_id:?} is not the active Android instance");
        }
        let instance_id = InstanceId::new(instance_id).map_err(anyhow::Error::msg)?;
        let response = match method {
            "plugin_parameters" => {
                let (schema, values) = plugin_parameters(self.runtime.0, &mut self.instance.0)?;
                ControlResponse::PluginParameters {
                    instance_id,
                    schema: Box::new(schema),
                    values,
                }
            }
            "set_plugin_parameter" => {
                let parameter_index = params
                    .get("parameter_index")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|index| u32::try_from(index).ok())
                    .context("plugin parameter command has an invalid parameter_index")?;
                let value = params
                    .get("value")
                    .and_then(serde_json::Value::as_f64)
                    .context("plugin parameter command has an invalid value")?;
                let value = set_plugin_parameter(
                    self.runtime.0,
                    &mut self.instance.0,
                    parameter_index,
                    value,
                )?;
                ControlResponse::PluginParameterSet {
                    instance_id,
                    parameter_index,
                    value,
                }
            }
            _ => bail!("unknown plugin parameter command {method:?}"),
        };
        serde_json::to_value(response).context("serializing plugin parameter response")
    }

    fn plugin_state_command(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut store = PluginStateStore::new(Some(&self.data_root))?;
        match method {
            "list_presets" => Ok(serde_json::to_value(store.list_presets(&self.plugin_id)?)?),
            "preset" => {
                let preset_id = params
                    .get("preset_id")
                    .and_then(serde_json::Value::as_str)
                    .context("preset command is missing preset_id")?;
                Ok(serde_json::to_value(
                    store.preset(&self.plugin_id, preset_id)?,
                )?)
            }
            "materialize" => {
                let sound_id = params
                    .get("sound_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                if let Some(sound_id) = sound_id.as_deref()
                    && !self
                        .catalog
                        .presets
                        .iter()
                        .any(|preset| preset.id == sound_id)
                {
                    bail!("plugin does not expose sound {sound_id:?}");
                }
                let mut isolated = self
                    .runtime
                    .0
                    .create_instance_with_resource_overrides(&self.resource_overrides)?;
                if let Some(sound_id) = sound_id.as_deref() {
                    isolated.load_preset(sound_id)?;
                }
                let bytes = isolated.save_state()?;
                let state = store.put(
                    &self.plugin_id,
                    &self.plugin_version,
                    self.runtime.0.manifest().state_version,
                    sound_id,
                    &bytes,
                )?;
                Ok(serde_json::to_value(state)?)
            }
            _ => bail!("unknown plugin state command {method:?}"),
        }
    }

    fn draft_state(
        &self,
        draft_id: u64,
        original_program_id: Option<String>,
        prepared: &PreparedProgram,
        editor: ProgramEditorView,
        dirty: bool,
    ) -> Result<ProgramDraftState> {
        prepared
            .validate()
            .context("validating Android prepared program")?;
        editor
            .validate()
            .context("validating Android program editor")?;
        Ok(ProgramDraftState {
            draft_id,
            instance_id: InstanceId::new("android-main").map_err(anyhow::Error::msg)?,
            original_program_id,
            name: prepared.document.name.clone(),
            preview_sound_id: prepared.preview_sound_id.clone(),
            storage_path: prepared.storage_path.clone(),
            artifacts: prepared.artifacts.clone(),
            document_json: serde_json::to_string(&prepared.document)
                .context("serializing Android program draft")?,
            editor,
            dirty,
        })
    }

    fn active_draft(&self, draft_id: u64) -> Result<ProgramDraftState> {
        self.program_draft
            .as_ref()
            .filter(|draft| draft.draft_id == draft_id)
            .cloned()
            .context("program draft is missing or no longer valid")
    }

    fn begin_program_edit(&mut self, program_id: Option<String>) -> Result<()> {
        if self.program_draft.is_some() {
            bail!("another program edit is already active");
        }
        if !self.instance.0.supports_program_editing() {
            bail!(
                "{} does not expose the RackForge program editor",
                self.plugin_name
            );
        }
        let prepared = self
            .instance
            .0
            .begin_program_edit(&ProgramEditRequest::new(program_id.clone()))
            .context("beginning program editing")?;
        let editor = self
            .instance
            .0
            .program_editor_view(&prepared.document)
            .context("building the program editor")?;
        if !self
            .instance
            .0
            .preview_program(&prepared)
            .context("previewing the program draft")?
        {
            bail!("plugin rejected the program preview");
        }
        let draft_id = self.next_program_draft_id;
        self.next_program_draft_id = self.next_program_draft_id.saturating_add(1);
        let draft = self.draft_state(draft_id, program_id, &prepared, editor, false)?;
        self.program_previous_sound_id = Some(self.selected_sound_id.clone());
        self.program_draft = Some(draft);
        Ok(())
    }

    fn edit_program_field(
        &mut self,
        draft_id: u64,
        field_id: String,
        value: ProgramEditorValue,
        preview: bool,
    ) -> Result<()> {
        let draft = self.active_draft(draft_id)?;
        let document: ProgramDocument = serde_json::from_str(&draft.document_json)
            .context("parsing the stored Android program draft")?;
        let prepared = self
            .instance
            .0
            .apply_program_edit(&ProgramFieldEditRequest {
                schema_version: PROGRAM_EDITOR_SCHEMA_VERSION,
                document,
                field_id,
                value,
            })
            .context("applying the program field edit")?;
        let editor = self
            .instance
            .0
            .program_editor_view(&prepared.document)
            .context("refreshing the program editor")?;
        if !self
            .instance
            .0
            .preview_program(&prepared)
            .context("previewing the edited program")?
        {
            bail!("plugin rejected the edited program preview");
        }
        if !preview {
            self.program_draft = Some(self.draft_state(
                draft_id,
                draft.original_program_id,
                &prepared,
                editor,
                true,
            )?);
        }
        Ok(())
    }

    fn replace_program_document(
        &mut self,
        draft: ProgramDraftState,
        document: ProgramDocument,
    ) -> Result<()> {
        let confirmed: ProgramDocument = serde_json::from_str(&draft.document_json)
            .context("parsing the confirmed Android program draft")?;
        if document.id != confirmed.id || document.plugin_id != confirmed.plugin_id {
            bail!("program identity cannot change during editing");
        }
        let prepared = self
            .instance
            .0
            .prepare_program_save(&document)
            .context("preparing the edited program")?;
        let editor = self
            .instance
            .0
            .program_editor_view(&prepared.document)
            .context("refreshing the program editor")?;
        if !self
            .instance
            .0
            .preview_program(&prepared)
            .context("previewing the edited program")?
        {
            bail!("plugin rejected the edited program preview");
        }
        self.program_draft = Some(self.draft_state(
            draft.draft_id,
            draft.original_program_id,
            &prepared,
            editor,
            true,
        )?);
        Ok(())
    }

    fn set_program_name(&mut self, draft_id: u64, name: String) -> Result<()> {
        let draft = self.active_draft(draft_id)?;
        let mut document: ProgramDocument = serde_json::from_str(&draft.document_json)
            .context("parsing the stored Android program draft")?;
        document.name = name;
        self.replace_program_document(draft, document)
    }

    fn restore_program_preview(&mut self, draft_id: u64) -> Result<()> {
        let draft = self.active_draft(draft_id)?;
        let document: ProgramDocument = serde_json::from_str(&draft.document_json)
            .context("parsing the stored Android program draft")?;
        let prepared = self
            .instance
            .0
            .prepare_program_save(&document)
            .context("restoring the confirmed program draft")?;
        if !self
            .instance
            .0
            .preview_program(&prepared)
            .context("restoring the program preview")?
        {
            bail!("plugin rejected the restored program preview");
        }
        Ok(())
    }

    fn restore_previous_program(&mut self) -> Result<()> {
        let previous = self
            .program_previous_sound_id
            .clone()
            .context("program audition has no previous sound")?;
        self.instance
            .0
            .reset()
            .context("resetting the program audition")?;
        self.instance
            .0
            .load_preset(&previous)
            .with_context(|| format!("restoring preset {previous:?}"))?;
        self.selected_sound_id = previous;
        Ok(())
    }

    fn save_program(&mut self, draft_id: u64) -> Result<()> {
        let draft = self.active_draft(draft_id)?;
        let document: ProgramDocument = serde_json::from_str(&draft.document_json)
            .context("parsing the stored Android program draft")?;
        let prepared = self
            .instance
            .0
            .prepare_program_save(&document)
            .context("preparing the Android program for saving")?;
        PluginStorage::new(&self.data_root)
            .save_prepared_program(&prepared)
            .context("saving the Android program")?;
        self.instance
            .0
            .install_program(&prepared)
            .context("installing the saved Android program")?;
        self.catalog = self
            .instance
            .0
            .preset_catalog()
            .context("refreshing the Android program catalog")?;
        self.restore_previous_program()?;
        self.program_draft = None;
        self.program_previous_sound_id = None;
        Ok(())
    }

    fn cancel_program_edit(&mut self, draft_id: u64) -> Result<()> {
        self.active_draft(draft_id)?;
        self.restore_previous_program()?;
        self.program_draft = None;
        self.program_previous_sound_id = None;
        Ok(())
    }

    fn apply_program_web_command(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<()> {
        let draft_id = || {
            params
                .get("draft_id")
                .and_then(serde_json::Value::as_u64)
                .context("program command is missing draft_id")
        };
        match method {
            "plugin.begin_program_edit" => self.begin_program_edit(
                params
                    .get("program_id")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            ),
            "plugin.edit_program_field" => self.edit_program_field(
                draft_id()?,
                params
                    .get("field_id")
                    .and_then(serde_json::Value::as_str)
                    .context("program field edit is missing field_id")?
                    .to_owned(),
                serde_json::from_value(
                    params
                        .get("value")
                        .cloned()
                        .context("program field edit is missing value")?,
                )
                .context("parsing program editor value")?,
                params
                    .get("preview")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            ),
            "plugin.set_program_name" => self.set_program_name(
                draft_id()?,
                params
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .context("program name command is missing name")?
                    .to_owned(),
            ),
            "plugin.restore_program_preview" => self.restore_program_preview(draft_id()?),
            "plugin.save_program" => self.save_program(draft_id()?),
            "plugin.cancel_program" => self.cancel_program_edit(draft_id()?),
            _ => bail!("method {method:?} is not a program editing command"),
        }
    }

    fn web_context_json(&self) -> String {
        let sounds = self
            .catalog
            .presets
            .iter()
            .map(|preset| {
                serde_json::json!({
                    "id": preset.id,
                    "name": preset.name,
                    "bank": preset.bank,
                    "detail": preset.description,
                    "editable": preset.editable,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({
            "protocol": "rackforge.plugin.web@1",
            "kind": "context",
            "surface": "play",
            "instance": {
                "instance_id": "android-main",
                "plugin_id": self.plugin_id,
                "plugin_name": self.plugin_name,
                "plugin_version": self.plugin_version,
                "ui_layouts": ["little@1"],
                "config_available": self.config_web_entry.is_some(),
                "config_web_entry": self.config_web_entry,
                "sounds": sounds,
                "selected_sound_id": self.selected_sound_id,
            },
            "program_draft": &self.program_draft,
            "audition": self.program_draft.as_ref().map(|draft| serde_json::json!({
                "lease_id": draft.draft_id,
                "instance_id": "android-main",
                "previous_sound_id": self.program_previous_sound_id,
            })),
            "host": {
                "active_mode": "play",
                "master_level": 0,
                "master_pan": 0,
            },
            "resources": self.resource_requirements,
        })
        .to_string()
    }

    fn render(&mut self, frames: u32, output: &mut [f32]) -> Result<()> {
        if frames == 0 || frames > MAX_FRAMES {
            bail!("invalid Android audio block size {frames}");
        }
        if output.len() != frames as usize * 2 {
            bail!("invalid Android stereo output buffer");
        }
        if let Ok(mut queue) = midi_queue().try_lock() {
            self.midi.extend(queue.drain(..));
        }
        self.instance
            .0
            .process_interleaved(&[], output, frames, 0, 2, &self.midi, &[])?;
        self.midi.clear();
        Ok(())
    }
}

fn engine() -> &'static Mutex<Option<AndroidEngine>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

fn audio() -> &'static Mutex<Option<NativeAudioOutput>> {
    AUDIO.get_or_init(|| Mutex::new(None))
}

fn midi_queue() -> &'static Mutex<VecDeque<MidiEventV1>> {
    MIDI_QUEUE.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_PENDING_MIDI_EVENTS)))
}

fn controller_menu() -> &'static Mutex<AndroidControllerMenu> {
    CONTROLLER_MENU.get_or_init(|| Mutex::new(AndroidControllerMenu::default()))
}

fn apply_master_control(target: HostControlTarget, value: u8) {
    match target {
        HostControlTarget::MasterLevel => {
            MASTER_LEVEL_TARGET_BITS.store(
                MasterLevel::from_midi(value).amplitude().to_bits(),
                Ordering::Relaxed,
            );
        }
        HostControlTarget::MasterPan => {
            let (left, right) = MasterPan::from_midi_with_center_snap(value).balance();
            MASTER_PAN_LEFT_TARGET_BITS.store(left.to_bits(), Ordering::Relaxed);
            MASTER_PAN_RIGHT_TARGET_BITS.store(right.to_bits(), Ordering::Relaxed);
        }
    }
}

fn smooth_master_sample(current: &mut f32, target: f32) {
    let difference = target - *current;
    *current = if difference.abs() < 0.000_01 {
        target
    } else {
        *current + difference * MASTER_SMOOTHING_FACTOR
    };
}

fn enqueue_midi(bytes: &[u8]) {
    if bytes.is_empty() || bytes.len() > 3 {
        return;
    }
    let mut data = [0_u8; 3];
    data[..bytes.len()].copy_from_slice(bytes);
    if let Ok(mut queue) = midi_queue().lock() {
        if queue.len() >= MAX_PENDING_MIDI_EVENTS {
            MIDI_DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
            return;
        }
        queue.push_back(MidiEventV1 {
            frame: 0,
            length: bytes.len() as u8,
            data,
        });
    }
}

fn release_all_midi_notes() {
    let packets = panic_packets(PanicScope::AllChannels);
    if let Ok(mut queue) = midi_queue().lock() {
        // A disconnect panic must never be rejected by a queue full of ordinary
        // controller traffic. Make room while keeping the queue strictly
        // bounded; the final panic releases anything an evicted message could
        // otherwise leave sounding.
        let keep = MAX_PENDING_MIDI_EVENTS.saturating_sub(packets.len());
        let evicted = queue.len().saturating_sub(keep);
        for _ in 0..evicted {
            queue.pop_front();
        }
        if evicted > 0 {
            MIDI_DROPPED_EVENTS.fetch_add(evicted as u64, Ordering::Relaxed);
        }
        for packet in packets {
            queue.push_back(MidiEventV1 {
                frame: 0,
                length: packet.length,
                data: packet.data,
            });
        }
        MIDI_PANIC_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

fn aaudio_error(operation: &str, result: i32) -> anyhow::Error {
    let detail = unsafe {
        let text = AAudio_convertResultToText(result);
        if text.is_null() {
            format!("status {result}")
        } else {
            CStr::from_ptr(text).to_string_lossy().into_owned()
        }
    };
    anyhow::anyhow!("{operation}: {detail} ({result})")
}

fn check_aaudio(operation: &str, result: i32) -> Result<()> {
    if result == AAUDIO_OK {
        Ok(())
    } else {
        Err(aaudio_error(operation, result))
    }
}

fn open_aaudio_stream(
    device_id: i32,
    sharing_mode: i32,
    performance_mode: i32,
    buffer_bursts: i32,
    frames_per_callback: i32,
    user_data: *mut c_void,
) -> Result<*mut AAudioStream> {
    let mut builder = ptr::null_mut();
    unsafe {
        check_aaudio(
            "creating AAudio stream builder",
            AAudio_createStreamBuilder(&mut builder),
        )?;
        if builder.is_null() {
            bail!("AAudio returned a null stream builder");
        }
        AAudioStreamBuilder_setDeviceId(builder, device_id);
        AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_OUTPUT);
        AAudioStreamBuilder_setSharingMode(builder, sharing_mode);
        AAudioStreamBuilder_setPerformanceMode(builder, performance_mode);
        AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_FLOAT);
        AAudioStreamBuilder_setChannelCount(builder, 2);
        AAudioStreamBuilder_setSampleRate(builder, SAMPLE_RATE as i32);
        if frames_per_callback > 0 {
            AAudioStreamBuilder_setFramesPerDataCallback(builder, frames_per_callback);
        }
        AAudioStreamBuilder_setDataCallback(builder, Some(render_callback), user_data);
        AAudioStreamBuilder_setErrorCallback(builder, Some(error_callback), user_data);

        let mut stream = ptr::null_mut();
        let open_result = AAudioStreamBuilder_openStream(builder, &mut stream);
        let _ = AAudioStreamBuilder_delete(builder);
        if open_result != AAUDIO_OK {
            return Err(aaudio_error("opening AAudio output", open_result));
        }
        if stream.is_null() {
            bail!("AAudio returned a null output stream");
        }
        let frames_per_burst = AAudioStream_getFramesPerBurst(stream);
        if frames_per_burst > 0 {
            let requested = frames_per_burst.saturating_mul(buffer_bursts.max(2));
            let _ = AAudioStream_setBufferSizeInFrames(stream, requested);
        }
        let start_result = AAudioStream_requestStart(stream);
        if start_result != AAUDIO_OK {
            let _ = AAudioStream_close(stream);
            return Err(aaudio_error("starting AAudio output", start_result));
        }
        Ok(stream)
    }
}

unsafe extern "C" fn error_callback(
    _stream: *mut AAudioStream,
    _user_data: *mut c_void,
    error: i32,
) {
    AUDIO_ERROR.store(error, Ordering::Release);
}

fn audio_status_json() -> String {
    let guard = match audio().lock() {
        Ok(guard) => guard,
        Err(_) => {
            return serde_json::json!({"running": false, "error": "audio lock poisoned"})
                .to_string();
        }
    };
    let Some(output) = guard.as_ref() else {
        return serde_json::json!({
            "running": false,
            "midi_dropped_events": MIDI_DROPPED_EVENTS.load(Ordering::Relaxed),
            "midi_panic_count": MIDI_PANIC_COUNT.load(Ordering::Relaxed),
        })
        .to_string();
    };
    let callback_count = AUDIO_CALLBACK_COUNT.load(Ordering::Relaxed);
    let callback_frames = AUDIO_CALLBACK_FRAMES.load(Ordering::Relaxed);
    let callback_nanos = AUDIO_CALLBACK_TOTAL_NANOS.load(Ordering::Relaxed);
    let average_frames = if callback_count == 0 {
        0.0
    } else {
        callback_frames as f64 / callback_count as f64
    };
    let average_callback_micros = if callback_count == 0 {
        0.0
    } else {
        callback_nanos as f64 / callback_count as f64 / 1_000.0
    };
    let callback_budget_micros = average_frames / SAMPLE_RATE * 1_000_000.0;
    let callback_load_percent = if callback_budget_micros == 0.0 {
        0.0
    } else {
        average_callback_micros / callback_budget_micros * 100.0
    };
    // SAFETY: AUDIO owns a live stream while the mutex guard is held.
    unsafe {
        serde_json::json!({
            "running": true,
            "sample_rate": AAudioStream_getSampleRate(output.stream),
            "frames_per_burst": AAudioStream_getFramesPerBurst(output.stream),
            "frames_per_data_callback": AAudioStream_getFramesPerDataCallback(output.stream),
            "buffer_size_frames": AAudioStream_getBufferSizeInFrames(output.stream),
            "buffer_capacity_frames": AAudioStream_getBufferCapacityInFrames(output.stream),
            "xruns": AAudioStream_getXRunCount(output.stream),
            "sharing_mode": AAudioStream_getSharingMode(output.stream),
            "performance_mode": AAudioStream_getPerformanceMode(output.stream),
            "pending_error": AUDIO_ERROR.load(Ordering::Acquire),
            "midi_dropped_events": MIDI_DROPPED_EVENTS.load(Ordering::Relaxed),
            "midi_panic_count": MIDI_PANIC_COUNT.load(Ordering::Relaxed),
            "engine_lock_misses": AUDIO_ENGINE_LOCK_MISSES.load(Ordering::Relaxed),
            "render_errors": AUDIO_RENDER_ERRORS.load(Ordering::Relaxed),
            "nonfinite_samples": AUDIO_NONFINITE_SAMPLES.load(Ordering::Relaxed),
            "clipped_samples": AUDIO_CLIPPED_SAMPLES.load(Ordering::Relaxed),
            "render_queue_frames": output.callback_context.queue.queued_frames(),
            "render_queue_underruns": AUDIO_RENDER_QUEUE_UNDERRUNS.load(Ordering::Relaxed),
            "render_queue_underrun_frames": AUDIO_RENDER_QUEUE_UNDERRUN_FRAMES.load(Ordering::Relaxed),
            "render_thread_priority_result": AUDIO_RENDER_THREAD_PRIORITY_RESULT.load(Ordering::Relaxed),
            "callback_count": callback_count,
            "average_callback_us": average_callback_micros,
            "maximum_callback_us": AUDIO_CALLBACK_MAX_NANOS.load(Ordering::Relaxed) as f64 / 1_000.0,
            "callback_budget_us": callback_budget_micros,
            "callback_load_percent": callback_load_percent,
        })
        .to_string()
    }
}

unsafe extern "C" fn render_callback(
    _stream: *mut AAudioStream,
    user_data: *mut c_void,
    audio_data: *mut c_void,
    num_frames: i32,
) -> i32 {
    if audio_data.is_null() || num_frames <= 0 {
        return AAUDIO_CALLBACK_RESULT_CONTINUE;
    }
    let sample_count = num_frames as usize * 2;
    let started = Instant::now();
    // SAFETY: AAudio supplies a writable interleaved stereo float buffer for
    // exactly num_frames because that format was fixed on the builder.
    let output = unsafe { slice::from_raw_parts_mut(audio_data.cast::<f32>(), sample_count) };
    output.fill(0.0);
    if num_frames as u32 > MAX_FRAMES {
        return AAUDIO_CALLBACK_RESULT_CONTINUE;
    }
    let rendered_frames = if user_data.is_null() {
        0
    } else {
        // SAFETY: NativeAudioOutput owns the boxed context until after AAudio
        // has stopped and closed the stream.
        let context = unsafe { &*user_data.cast::<AudioCallbackContext>() };
        let frames = context.queue.pop(output);
        context.render_thread.unpark();
        frames
    };
    let requested_frames = num_frames as usize;
    if rendered_frames < requested_frames {
        AUDIO_RENDER_QUEUE_UNDERRUNS.fetch_add(1, Ordering::Relaxed);
        AUDIO_RENDER_QUEUE_UNDERRUN_FRAMES.fetch_add(
            (requested_frames - rendered_frames) as u64,
            Ordering::Relaxed,
        );
        conceal_audio_dropout(&mut output[rendered_frames * 2..]);
    }
    let output_gain = f32::from_bits(OUTPUT_GAIN_BITS.load(Ordering::Relaxed));
    let level_target = f32::from_bits(MASTER_LEVEL_TARGET_BITS.load(Ordering::Relaxed));
    let pan_left_target = f32::from_bits(MASTER_PAN_LEFT_TARGET_BITS.load(Ordering::Relaxed));
    let pan_right_target = f32::from_bits(MASTER_PAN_RIGHT_TARGET_BITS.load(Ordering::Relaxed));
    let mut level = f32::from_bits(MASTER_LEVEL_CURRENT_BITS.load(Ordering::Relaxed));
    let mut pan_left = f32::from_bits(MASTER_PAN_LEFT_CURRENT_BITS.load(Ordering::Relaxed));
    let mut pan_right = f32::from_bits(MASTER_PAN_RIGHT_CURRENT_BITS.load(Ordering::Relaxed));
    let complete = rendered_frames == requested_frames;
    let recover = complete && AUDIO_RECOVERY_RAMP_PENDING.swap(0, Ordering::AcqRel) != 0;
    let recovery_frames = output.len().div_ceil(2).min(DROPOUT_FADE_FRAMES).max(1);
    let mut nonfinite = 0_u64;
    let mut clipped = 0_u64;
    for (index, frame) in output.chunks_exact_mut(2).enumerate() {
        smooth_master_sample(&mut level, level_target);
        smooth_master_sample(&mut pan_left, pan_left_target);
        smooth_master_sample(&mut pan_right, pan_right_target);
        let recovery_gain = if recover && index < recovery_frames {
            (index + 1) as f32 / recovery_frames as f32
        } else {
            1.0
        };
        let gain = output_gain * level * recovery_gain;
        let left = if index < rendered_frames {
            frame[0] * gain * pan_left
        } else {
            frame[0]
        };
        let right = if index < rendered_frames {
            frame[1] * gain * pan_right
        } else {
            frame[1]
        };
        for (sample, value) in frame.iter_mut().zip([left, right]) {
            if !value.is_finite() {
                *sample = 0.0;
                nonfinite += 1;
            } else {
                if value.abs() > 1.0 {
                    clipped += 1;
                }
                *sample = value.clamp(-1.0, 1.0);
            }
        }
    }
    AUDIO_NONFINITE_SAMPLES.fetch_add(nonfinite, Ordering::Relaxed);
    AUDIO_CLIPPED_SAMPLES.fetch_add(clipped, Ordering::Relaxed);
    MASTER_LEVEL_CURRENT_BITS.store(level.to_bits(), Ordering::Relaxed);
    MASTER_PAN_LEFT_CURRENT_BITS.store(pan_left.to_bits(), Ordering::Relaxed);
    MASTER_PAN_RIGHT_CURRENT_BITS.store(pan_right.to_bits(), Ordering::Relaxed);
    if let Some(last) = output.chunks_exact(2).last() {
        AUDIO_LAST_LEFT_BITS.store(last[0].to_bits(), Ordering::Relaxed);
        AUDIO_LAST_RIGHT_BITS.store(last[1].to_bits(), Ordering::Relaxed);
    }
    let elapsed = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    AUDIO_CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
    AUDIO_CALLBACK_FRAMES.fetch_add(num_frames as u64, Ordering::Relaxed);
    AUDIO_CALLBACK_TOTAL_NANOS.fetch_add(elapsed, Ordering::Relaxed);
    AUDIO_CALLBACK_MAX_NANOS.fetch_max(elapsed, Ordering::Relaxed);
    AAUDIO_CALLBACK_RESULT_CONTINUE
}

fn conceal_audio_dropout(output: &mut [f32]) {
    let left = f32::from_bits(AUDIO_LAST_LEFT_BITS.load(Ordering::Relaxed));
    let right = f32::from_bits(AUDIO_LAST_RIGHT_BITS.load(Ordering::Relaxed));
    let fade_frames = output.len().div_ceil(2).min(DROPOUT_FADE_FRAMES).max(1);
    for (index, frame) in output.chunks_exact_mut(2).enumerate() {
        let gain = if index < fade_frames {
            1.0 - (index + 1) as f32 / fade_frames as f32
        } else {
            0.0
        };
        frame[0] = left * gain;
        frame[1] = right * gain;
    }
    AUDIO_RECOVERY_RAMP_PENDING.store(1, Ordering::Release);
}

fn java_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Result<String> {
    Ok(env.get_string(&value)?.into())
}

fn report(env: &mut JNIEnv<'_>, error: anyhow::Error) {
    let _ = env.throw_new("java/lang/IllegalStateException", format!("{error:#}"));
}

fn package_descriptor(
    package: &PluginPackage,
    active_plugin: Option<&(String, String)>,
) -> serde_json::Value {
    let manifest = package.manifest();
    let play_entry = manifest.web_ui.as_ref().and_then(|web| {
        web.surfaces
            .iter()
            .find(|surface| surface.kind == WebSurfaceKind::Play)
            .map(|surface| surface.entry.as_str())
    });
    let config_entry = manifest.web_ui.as_ref().and_then(|web| {
        web.surfaces
            .iter()
            .find(|surface| surface.kind == WebSurfaceKind::Config)
            .map(|surface| surface.entry.as_str())
    });
    let portable = manifest.portable_component().is_some();
    let compatible = manifest.kind == PluginKind::Instrument && portable && play_entry.is_some();
    let incompatibility = if manifest.kind != PluginKind::Instrument {
        Some("Android PLAY currently supports instrument plugins only")
    } else if !portable {
        Some("Android requires a portable wasm-v1 component")
    } else if play_entry.is_none() {
        Some("The plugin does not provide a PLAY Web surface")
    } else {
        None
    };
    let root = package.root();
    serde_json::json!({
        "plugin_id": manifest.id,
        "plugin_name": manifest.name,
        "version": manifest.version,
        "kind": manifest.kind,
        "portable": portable,
        "compatible": compatible,
        "incompatibility": incompatibility,
        "package_root": root.to_string_lossy(),
        "web_entry": play_entry,
        "config_web_entry": config_entry,
        "web_api_version": manifest.web_ui.as_ref().map_or(0, |web| web.api_version),
        "branding": manifest.branding,
        "resources": manifest.resources,
        "active": active_plugin.is_some_and(|(id, version)| {
            id == &manifest.id && version == &manifest.version
        }),
    })
}

fn installed_plugins_json(store_root: &Path) -> Result<String> {
    let _ = cleanup_uninstall_tombstones(store_root);
    let packages_root = store_root.join("packages");
    std::fs::create_dir_all(&packages_root)
        .with_context(|| format!("creating {}", packages_root.display()))?;
    let active_plugin = engine().lock().ok().and_then(|guard| {
        guard
            .as_ref()
            .map(|engine| (engine.plugin_id.clone(), engine.plugin_version.clone()))
    });
    let mut versions = BTreeMap::<String, Vec<(semver::Version, serde_json::Value)>>::new();
    let mut warnings = Vec::new();
    for plugin in std::fs::read_dir(&packages_root)
        .with_context(|| format!("reading {}", packages_root.display()))?
        .flatten()
    {
        if !plugin.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        for version in std::fs::read_dir(plugin.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            if !version.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            match PluginPackage::open(version.path()) {
                Ok(package) => match semver::Version::parse(&package.manifest().version) {
                    Ok(parsed) => versions
                        .entry(package.manifest().id.clone())
                        .or_default()
                        .push((parsed, package_descriptor(&package, active_plugin.as_ref()))),
                    Err(error) => warnings.push(format!(
                        "{}: invalid plugin version: {error}",
                        version.path().display()
                    )),
                },
                Err(error) => warnings.push(format!("{}: {error:#}", version.path().display())),
            }
        }
    }
    let mut plugins = Vec::new();
    for (_, mut candidates) in versions {
        candidates.sort_by(|left, right| left.0.cmp(&right.0));
        let installed_versions = candidates
            .iter()
            .map(|(version, _)| version.to_string())
            .collect::<Vec<_>>();
        let active_version = candidates.iter().find_map(|(version, descriptor)| {
            descriptor["active"]
                .as_bool()
                .unwrap_or(false)
                .then(|| version.to_string())
        });
        let (_, mut latest) = candidates.pop().expect("plugin version group is not empty");
        let latest_version = latest["version"].as_str().unwrap_or_default().to_owned();
        latest["active"] = active_version
            .as_deref()
            .is_some_and(|active| active == latest_version.as_str())
            .into();
        latest["active_version"] = active_version.into();
        latest["installed_versions"] = installed_versions.into();
        plugins.push(latest);
    }
    plugins.sort_by(|left, right| {
        left["plugin_name"]
            .as_str()
            .cmp(&right["plugin_name"].as_str())
            .then_with(|| left["version"].as_str().cmp(&right["version"].as_str()))
    });
    Ok(serde_json::json!({"plugins": plugins, "warnings": warnings}).to_string())
}

fn result_string(env: &mut JNIEnv<'_>, result: Result<String>) -> jstring {
    match result.and_then(|value| Ok(env.new_string(value)?.into_raw())) {
        Ok(value) => value,
        Err(error) => {
            report(env, error);
            ptr::null_mut()
        }
    }
}

fn controller_plan_json(
    messages: Result<Vec<keylab_protocol::OutboundMessage>, String>,
) -> Result<String> {
    Ok(controller_plan_value(messages)?.to_string())
}

fn controller_plan_value(
    messages: Result<Vec<keylab_protocol::OutboundMessage>, String>,
) -> Result<serde_json::Value> {
    let messages = messages.map_err(anyhow::Error::msg)?;
    Ok(serde_json::Value::Array(
        messages
            .into_iter()
            .map(|message| {
                serde_json::json!({
                    "bytes": message.bytes,
                    "settle_after_ms": message.settle_after_ms,
                })
            })
            .collect(),
    ))
}

fn controller_play_sounds(catalog: &PresetCatalog) -> Vec<PlaySound> {
    let banks = catalog
        .banks
        .iter()
        .map(|bank| (bank.id.as_str(), bank.name.as_str()))
        .collect::<BTreeMap<_, _>>();
    catalog
        .presets
        .iter()
        .map(|preset| {
            let bank = preset
                .bank
                .as_deref()
                .and_then(|id| banks.get(id).copied())
                .unwrap_or("Factory");
            let detail = preset
                .category
                .as_deref()
                .or(preset.description.as_deref())
                .unwrap_or("Preset");
            PlaySound::new(&preset.id, &preset.name, bank, detail).editable(preset.editable)
        })
        .collect()
}

fn sync_controller_plugins(store_root: &Path) -> Result<()> {
    let catalog: serde_json::Value = serde_json::from_str(&installed_plugins_json(store_root)?)?;
    let active = engine()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
        .as_ref()
        .map(|engine| {
            (
                engine.package_root.to_string_lossy().into_owned(),
                engine.plugin_id.clone(),
                engine.plugin_name.clone(),
                engine.catalog.clone(),
                engine.selected_sound_id.clone(),
            )
        });
    let mut plugins = Vec::new();
    let mut metadata = BTreeMap::new();
    for descriptor in catalog["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|plugin| plugin["compatible"].as_bool().unwrap_or(false))
    {
        let Some(root) = descriptor["package_root"].as_str() else {
            continue;
        };
        let Some(plugin_id) = descriptor["plugin_id"].as_str() else {
            continue;
        };
        let Some(name) = descriptor["plugin_name"].as_str() else {
            continue;
        };
        let version = descriptor["version"].as_str().unwrap_or_default();
        plugins.push(PlayPlugin::new(plugin_id, plugin_id, name).config_available(false));
        metadata.insert(
            plugin_id.to_owned(),
            ControllerPluginInfo {
                root: root.to_owned(),
                name: name.to_owned(),
                version: version.to_owned(),
            },
        );
    }
    let active_instance_id = active.as_ref().map(|active| active.1.as_str());
    let mut controller = controller_menu()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))?;
    controller.plugins = metadata;
    controller.installed_plugins = plugins.clone();
    controller
        .menu
        .set_play_plugins(plugins, active_instance_id);
    if let Some((_root, plugin_id, name, catalog, selected_sound_id)) = active {
        controller.menu.sync_active_plugin(
            plugin_id.clone(),
            plugin_id,
            name,
            controller_play_sounds(&catalog),
            Some(&selected_sound_id),
        );
    }
    Ok(())
}

fn sync_controller_active_plugin() -> Result<()> {
    let (plugin_id, name, catalog, selected_sound_id) = engine()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
        .as_ref()
        .map(|engine| {
            (
                engine.plugin_id.clone(),
                engine.plugin_name.clone(),
                engine.catalog.clone(),
                engine.selected_sound_id.clone(),
            )
        })
        .context("RackForge engine is not initialized")?;
    let mut controller = controller_menu()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))?;
    controller.menu.sync_active_plugin(
        plugin_id.clone(),
        plugin_id,
        name,
        controller_play_sounds(&catalog),
        Some(&selected_sound_id),
    );
    Ok(())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabAcquirePlan(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = (|| -> Result<String> {
        let controller = controller_menu()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))?;
        let mut messages = keylab_protocol::acquire_messages().map_err(anyhow::Error::msg)?;
        messages.extend(
            keylab_protocol::render_messages(&controller.menu.render())
                .map_err(anyhow::Error::msg)?,
        );
        controller_plan_json(Ok(messages))
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabMatchesUsbDevice(
    _env: JNIEnv,
    _class: JClass,
    vendor_id: jint,
    product_id: jint,
) -> jboolean {
    let Ok(vendor_id) = u16::try_from(vendor_id) else {
        return JNI_FALSE;
    };
    let Ok(product_id) = u16::try_from(product_id) else {
        return JNI_FALSE;
    };
    if keylab_essential_mk3::controller::matches_usb_device(vendor_id, product_id) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabMatchesProductName(
    mut env: JNIEnv,
    _class: JClass,
    name: JString,
) -> jboolean {
    match java_string(&mut env, name) {
        Ok(name) if keylab_essential_mk3::controller::matches_product_name(&name) => JNI_TRUE,
        Ok(_) => JNI_FALSE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabMatchesEndpointName(
    mut env: JNIEnv,
    _class: JClass,
    name: JString,
) -> jboolean {
    match java_string(&mut env, name) {
        Ok(name) if keylab_essential_mk3::controller::matches_endpoint_name_hint(&name) => JNI_TRUE,
        Ok(_) => JNI_FALSE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabSyncPlugins(
    mut env: JNIEnv,
    _class: JClass,
    store_root: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        sync_controller_plugins(&store_root)
    })();
    match result {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabSyncActivePlugin(
    mut env: JNIEnv,
    _class: JClass,
) -> jboolean {
    match sync_controller_active_plugin() {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabSyncActiveMode(
    mut env: JNIEnv,
    _class: JClass,
    mode: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let mode = match java_string(&mut env, mode)?.as_str() {
            "idle" => ActiveMode::Idle,
            "live" => ActiveMode::Live,
            "play" => ActiveMode::Play,
            value => bail!("invalid Android controller mode {value:?}"),
        };
        controller_menu()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))?
            .menu
            .sync_active_mode(mode);
        Ok(())
    })();
    match result {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabRenderPlan(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = controller_menu()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))
        .and_then(|controller| {
            controller_plan_json(keylab_protocol::render_messages(&controller.menu.render()))
        });
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabRestorePlan(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    result_string(
        &mut env,
        controller_plan_json(keylab_protocol::restore_messages()),
    )
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabHandleMidi(
    mut env: JNIEnv,
    _class: JClass,
    status: jint,
    data_1: jint,
    data_2: jint,
) -> jstring {
    let message = [status as u8, data_1 as u8, data_2 as u8];
    let Some(event) = keylab_protocol::parse_input(&message) else {
        return ptr::null_mut();
    };
    let result = match event {
        keylab_protocol::ControllerEvent::Surface { input, phase } => controller_menu()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))
            .and_then(|mut controller| controller.handle_surface(input, phase)),
        keylab_protocol::ControllerEvent::HostControl { target, value } => {
            apply_master_control(target, value);
            controller_menu()
                .lock()
                .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))
                .and_then(|controller| controller.render_host_control(target, value))
        }
    };
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabPollLongPress(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = controller_menu()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))
        .and_then(|mut controller| controller.poll_long_press(Instant::now()));
    match result {
        Ok(Some(response)) => result_string(&mut env, Ok(response)),
        Ok(None) => ptr::null_mut(),
        Err(error) => result_string(&mut env, Err(error)),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_inspectPluginFile(
    mut env: JNIEnv,
    _class: JClass,
    archive_path: JString,
    store_root: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let archive_path = PathBuf::from(java_string(&mut env, archive_path)?);
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        let bytes = std::fs::read(&archive_path)
            .with_context(|| format!("reading selected plugin {}", archive_path.display()))?;
        let inspection =
            inspect_local_archive(&store_root, &bytes).context("validating the portable plugin")?;
        let branding = inspection.branding.as_ref().map(|branding| {
            serde_json::json!({
                "banner_data_url": format!(
                    "data:image/png;base64,{}",
                    STANDARD.encode(&branding.banner_png)
                ),
                "background_color": branding.background_color,
                "accent_color": branding.accent_color,
            })
        });
        Ok(serde_json::json!({
            "plugin_id": inspection.plugin_id,
            "plugin_name": inspection.plugin_name,
            "vendor": inspection.vendor,
            "version": inspection.version,
            "description": inspection.description,
            "kind": inspection.kind,
            "platform": inspection.platform,
            "portable": inspection.portable,
            "archive_bytes": inspection.archive_bytes,
            "branding": branding,
        })
        .to_string())
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_installPluginFile(
    mut env: JNIEnv,
    _class: JClass,
    archive_path: JString,
    store_root: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let archive_path = PathBuf::from(java_string(&mut env, archive_path)?);
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        let bytes = std::fs::read(&archive_path)
            .with_context(|| format!("reading selected plugin {}", archive_path.display()))?;
        let installed = install_local_archive(&store_root, &bytes)
            .context("validating and installing the portable plugin")?;
        let package = PluginPackage::open(&installed.path)
            .with_context(|| format!("opening installed plugin {}", installed.path.display()))?;
        let mut descriptor = package_descriptor(&package, None);
        descriptor["already_installed"] = installed.already_installed.into();
        descriptor["artifact_sha256"] = installed.record.artifact_sha256.into();
        Ok(descriptor.to_string())
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_installedPlugins(
    mut env: JNIEnv,
    _class: JClass,
    store_root: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        installed_plugins_json(&store_root)
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_uninstallPlugin(
    mut env: JNIEnv,
    _class: JClass,
    plugin_id: JString,
    store_root: JString,
    data_root: JString,
    delete_presets: jboolean,
    delete_plugin_data: jboolean,
) -> jstring {
    let result = (|| -> Result<String> {
        let plugin_id = java_string(&mut env, plugin_id)?;
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        let data_root = PathBuf::from(java_string(&mut env, data_root)?);
        let is_active = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
            .as_ref()
            .is_some_and(|active| active.plugin_id == plugin_id);
        let removed = uninstall_plugin(&store_root, &plugin_id)
            .context("removing the managed plugin package")?;
        if is_active {
            midi_queue()
                .lock()
                .map_err(|_| anyhow::anyhow!("MIDI queue lock poisoned"))?
                .clear();
            *engine()
                .lock()
                .map_err(|_| anyhow::anyhow!("engine lock poisoned"))? = None;
        }
        let delete_presets = delete_presets == JNI_TRUE;
        let delete_plugin_data = delete_plugin_data == JNI_TRUE;
        let user_data_cleanup = remove_plugin_user_data(
            &data_root,
            &plugin_id,
            PluginUserDataRemovalOptions {
                presets: delete_presets,
                plugin_data: delete_plugin_data,
            },
        );
        let (presets_deleted, plugin_data_deleted, user_data_cleanup_warning) =
            match user_data_cleanup {
                Ok(_) => (delete_presets, delete_plugin_data, None),
                Err(error) => (false, false, Some(error.to_string())),
            };
        Ok(serde_json::json!({
            "status":"uninstalled",
            "plugin_id":removed.plugin_id,
            "removed_versions":removed.removed_versions,
            "cleanup_pending":removed.cleanup_pending,
            "restart_requested":false,
            "user_data_preserved":!presets_deleted && !plugin_data_deleted,
            "presets_deleted":presets_deleted,
            "plugin_data_deleted":plugin_data_deleted,
            "user_data_cleanup_warning":user_data_cleanup_warning
        })
        .to_string())
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_activateInstalledPlugin(
    mut env: JNIEnv,
    _class: JClass,
    package_root: JString,
    store_root: JString,
    data_root: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let package_root =
            std::fs::canonicalize(PathBuf::from(java_string(&mut env, package_root)?))?;
        let packages_root = std::fs::canonicalize(
            PathBuf::from(java_string(&mut env, store_root)?).join("packages"),
        )?;
        if !package_root.starts_with(&packages_root) || package_root == packages_root {
            bail!("selected plugin is outside the RackForge package store");
        }
        let data_root = PathBuf::from(java_string(&mut env, data_root)?);
        let candidate = AndroidEngine::open_package(&package_root, data_root)?;
        midi_queue()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI queue lock poisoned"))?
            .clear();
        *engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))? = Some(candidate);
        Ok(())
    })();
    match result {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_sendMidiMessage(
    _env: JNIEnv,
    _class: JClass,
    status: jint,
    data_1: jint,
    data_2: jint,
    length: jint,
) {
    let bytes = [status as u8, data_1 as u8, data_2 as u8];
    if (1..=3).contains(&length) {
        enqueue_midi(&bytes[..length as usize]);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_releaseMidiNotes(
    _env: JNIEnv,
    _class: JClass,
) {
    release_all_midi_notes();
}

fn engine_string(env: &mut JNIEnv<'_>, value: impl FnOnce(&AndroidEngine) -> String) -> jstring {
    let result = (|| -> Result<String> {
        let guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        let engine = guard
            .as_ref()
            .context("RackForge engine is not initialized")?;
        Ok(value(engine))
    })();
    match result.and_then(|value| Ok(env.new_string(value)?.into_raw())) {
        Ok(value) => value,
        Err(error) => {
            report(env, error);
            ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_pluginPackageRoot(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    engine_string(&mut env, |engine| {
        engine.package_root.to_string_lossy().into_owned()
    })
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_pluginWebEntry(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    engine_string(&mut env, |engine| engine.web_entry.clone())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_pluginWebContext(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    engine_string(&mut env, AndroidEngine::web_context_json)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_selectPluginSound(
    mut env: JNIEnv,
    _class: JClass,
    sound_id: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let sound_id = java_string(&mut env, sound_id)?;
        let mut guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        guard
            .as_mut()
            .context("RackForge engine is not initialized")?
            .select_sound(&sound_id)
    })();
    match result {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_pluginProgramCommand(
    mut env: JNIEnv,
    _class: JClass,
    method: JString,
    params_json: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let method = java_string(&mut env, method)?;
        let params: serde_json::Value = serde_json::from_str(&java_string(&mut env, params_json)?)
            .context("parsing plugin program command parameters")?;
        let mut guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        let engine = guard
            .as_mut()
            .context("RackForge engine is not initialized")?;
        engine.apply_program_web_command(&method, &params)?;
        Ok(engine.web_context_json())
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_pluginParameterCommand(
    mut env: JNIEnv,
    _class: JClass,
    method: JString,
    params_json: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let method = java_string(&mut env, method)?;
        let params: serde_json::Value = serde_json::from_str(&java_string(&mut env, params_json)?)
            .context("parsing plugin parameter command")?;
        let mut guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        let engine = guard
            .as_mut()
            .context("RackForge engine is not initialized")?;
        Ok(engine
            .plugin_parameter_command(&method, &params)?
            .to_string())
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_pluginStateCommand(
    mut env: JNIEnv,
    _class: JClass,
    method: JString,
    params_json: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let method = java_string(&mut env, method)?;
        let params: serde_json::Value = serde_json::from_str(&java_string(&mut env, params_json)?)
            .context("parsing plugin state command parameters")?;
        let guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        let engine = guard
            .as_ref()
            .context("RackForge engine is not initialized")?;
        Ok(engine.plugin_state_command(&method, &params)?.to_string())
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_loadPluginResource(
    mut env: JNIEnv,
    _class: JClass,
    resource_id: JString,
    file_path: JString,
) -> jint {
    let result = (|| -> Result<jint> {
        let resource_id = java_string(&mut env, resource_id)?;
        let file_path = std::fs::canonicalize(PathBuf::from(java_string(&mut env, file_path)?))?;
        if !file_path.is_file() {
            bail!("selected plugin resource is not a file");
        }
        let (runtime, selected_sound_id, mut resources, plugin_id) = {
            let guard = engine()
                .lock()
                .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
            let current = guard
                .as_ref()
                .context("RackForge engine is not initialized")?;
            if !current.resource_requirements.iter().any(|resource| {
                resource.id == resource_id
                    && resource.kind == rackforge_plugin_api::ResourceKind::File
            }) {
                bail!("plugin does not declare file resource {resource_id:?}");
            }
            (
                current.runtime,
                current.selected_sound_id.clone(),
                current.resource_overrides.clone(),
                current.plugin_id.clone(),
            )
        };
        runtime.0.validate_resource_file(&resource_id, &file_path)?;
        resources.insert(resource_id.clone(), file_path);
        let replacement = (|| -> Result<_> {
            // Resource delivery must happen before the plugin publishes its dynamic catalog.
            // create_instance() finalizes that phase immediately; trying to load overrides on
            // the returned instance makes plugins correctly reject them as late resources.
            let mut replacement = runtime
                .0
                .create_instance_with_resource_overrides(&resources)?;
            let catalog = replacement.preset_catalog()?;
            let next_sound_id = catalog
                .presets
                .iter()
                .find(|preset| preset.id == selected_sound_id)
                .or_else(|| catalog.presets.first())
                .context("plugin exposes no playable preset after loading resources")?
                .id
                .clone();
            replacement.load_preset(&next_sound_id)?;
            replacement.activate(SAMPLE_RATE, MAX_FRAMES, 0, 2)?;
            Ok((replacement, catalog, next_sound_id))
        })();

        let (retired, load_status) = {
            let mut guard = engine()
                .lock()
                .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
            let current = guard
                .as_mut()
                .context("RackForge engine stopped while loading the resource")?;
            if current.plugin_id != plugin_id {
                bail!("active plugin changed while loading the resource");
            }
            current.resource_overrides = resources;
            match replacement {
                Ok((replacement, catalog, next_sound_id)) => {
                    current.catalog = catalog;
                    current.selected_sound_id = next_sound_id;
                    (
                        Some(std::mem::replace(
                            &mut current.instance,
                            SendablePluginInstance(replacement),
                        )),
                        1,
                    )
                }
                // A single individually installed resource may be valid but incomplete. Keep its
                // override so later resources can complete the set, and tell the Web UI that the
                // active instance was not rebuilt yet.
                Err(_) => (None, 2),
            }
        };
        drop(retired);
        Ok(load_status)
    })();
    match result {
        Ok(status) => status,
        Err(error) => {
            report(&mut env, error);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_importPluginResourceArchive(
    mut env: JNIEnv,
    _class: JClass,
    importer_id: JString,
    archive_path: JString,
    resource_root: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let importer_id = java_string(&mut env, importer_id)?;
        let archive_path = fs::canonicalize(PathBuf::from(java_string(&mut env, archive_path)?))?;
        let resource_root = PathBuf::from(java_string(&mut env, resource_root)?);
        if !archive_path.is_file() {
            bail!("selected resource archive is not a file");
        }
        let (runtime, selected_sound_id, mut resources, plugin_id) =
            {
                let guard = engine()
                    .lock()
                    .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
                let current = guard
                    .as_ref()
                    .context("RackForge engine is not initialized")?;
                if !current.resource_requirements.iter().any(|resource| {
                    resource.id == importer_id && !resource.import_targets.is_empty()
                }) {
                    bail!("plugin does not declare resource importer {importer_id:?}");
                }
                (
                    current.runtime,
                    current.selected_sound_id.clone(),
                    current.resource_overrides.clone(),
                    current.plugin_id.clone(),
                )
            };

        let imported = runtime
            .0
            .import_resource_archive(&importer_id, &archive_path)?;
        let plugin_root = resource_root.join(&plugin_id);
        fs::create_dir_all(&plugin_root)
            .with_context(|| format!("creating {}", plugin_root.display()))?;
        let mut installed_ids = Vec::with_capacity(imported.len());
        for (target_id, bytes) in &imported {
            let destination = plugin_root.join(format!("{target_id}.resource"));
            let temporary = plugin_root.join(format!(".{target_id}.resource-import"));
            fs::write(&temporary, bytes)
                .with_context(|| format!("writing {}", temporary.display()))?;
            fs::rename(&temporary, &destination)
                .with_context(|| format!("installing {}", destination.display()))?;
            resources.insert(target_id.clone(), destination);
            installed_ids.push(target_id.clone());
        }

        let replacement = (|| -> Result<_> {
            let mut replacement = runtime
                .0
                .create_instance_with_resource_overrides(&resources)?;
            let catalog = replacement.preset_catalog()?;
            let next_sound_id = catalog
                .presets
                .iter()
                .find(|preset| preset.id == selected_sound_id)
                .or_else(|| catalog.presets.first())
                .context("plugin exposes no playable preset after importing resources")?
                .id
                .clone();
            replacement.load_preset(&next_sound_id)?;
            replacement.activate(SAMPLE_RATE, MAX_FRAMES, 0, 2)?;
            Ok((replacement, catalog, next_sound_id))
        })();

        let (retired, activated) = {
            let mut guard = engine()
                .lock()
                .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
            let current = guard
                .as_mut()
                .context("RackForge engine stopped while importing resources")?;
            if current.plugin_id != plugin_id {
                bail!("active plugin changed while importing resources");
            }
            current.resource_overrides = resources;
            match replacement {
                Ok((replacement, catalog, next_sound_id)) => {
                    current.catalog = catalog;
                    current.selected_sound_id = next_sound_id;
                    (
                        Some(std::mem::replace(
                            &mut current.instance,
                            SendablePluginInstance(replacement),
                        )),
                        true,
                    )
                }
                Err(_) => (None, false),
            }
        };
        drop(retired);
        Ok(serde_json::json!({
            "stored": true,
            "activated": activated,
            "installed_resource_ids": installed_ids,
        })
        .to_string())
    })();
    match result.and_then(|value| Ok(env.new_string(value)?.into_raw())) {
        Ok(value) => value,
        Err(error) => {
            report(&mut env, error);
            ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_startNativeAudio(
    mut env: JNIEnv,
    _class: JClass,
    device_id: jint,
    latency_mode: jint,
) -> jboolean {
    let result = (|| -> Result<()> {
        if engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
            .is_none()
        {
            bail!("RackForge engine is not initialized");
        }
        AUDIO_ERROR.store(AAUDIO_OK, Ordering::Release);
        AUDIO_CALLBACK_COUNT.store(0, Ordering::Relaxed);
        AUDIO_CALLBACK_FRAMES.store(0, Ordering::Relaxed);
        AUDIO_CALLBACK_TOTAL_NANOS.store(0, Ordering::Relaxed);
        AUDIO_CALLBACK_MAX_NANOS.store(0, Ordering::Relaxed);
        AUDIO_ENGINE_LOCK_MISSES.store(0, Ordering::Relaxed);
        AUDIO_RENDER_ERRORS.store(0, Ordering::Relaxed);
        AUDIO_NONFINITE_SAMPLES.store(0, Ordering::Relaxed);
        AUDIO_CLIPPED_SAMPLES.store(0, Ordering::Relaxed);
        AUDIO_RENDER_QUEUE_UNDERRUNS.store(0, Ordering::Relaxed);
        AUDIO_RENDER_QUEUE_UNDERRUN_FRAMES.store(0, Ordering::Relaxed);
        AUDIO_RENDER_THREAD_PRIORITY_RESULT.store(0, Ordering::Relaxed);
        AUDIO_RECOVERY_RAMP_PENDING.store(0, Ordering::Relaxed);
        AUDIO_LAST_LEFT_BITS.store(0.0_f32.to_bits(), Ordering::Relaxed);
        AUDIO_LAST_RIGHT_BITS.store(0.0_f32.to_bits(), Ordering::Relaxed);
        let candidate = NativeAudioOutput::open(device_id, latency_mode)?;
        *audio()
            .lock()
            .map_err(|_| anyhow::anyhow!("audio lock poisoned"))? = Some(candidate);
        Ok(())
    })();
    match result {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_nativeAudioStatus(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    result_string(&mut env, Ok(audio_status_json()))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_growNativeAudioBuffer(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    audio()
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(NativeAudioOutput::grow_buffer))
        .unwrap_or(false)
        .into()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_pollNativeAudioError(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    AUDIO_ERROR.swap(AAUDIO_OK, Ordering::AcqRel)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_setNativeOutputGain(
    _env: JNIEnv,
    _class: JClass,
    gain_db: jint,
) {
    let gain_db = gain_db.clamp(0, 12) as f32;
    OUTPUT_GAIN_BITS.store(10.0_f32.powf(gain_db / 20.0).to_bits(), Ordering::Relaxed);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_stopNativeAudio(
    _env: JNIEnv,
    _class: JClass,
) {
    if let Ok(mut guard) = audio().lock() {
        *guard = None;
    }
    AUDIO_ERROR.store(AAUDIO_OK, Ordering::Release);
}
