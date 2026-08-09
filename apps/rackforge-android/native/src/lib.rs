use anyhow::{Context, Result, bail};
use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jstring};
use keylab_essential_mk3::protocol as keylab_protocol;
use rackforge_core::{
    LoadedPlugin, PluginInstance, PluginPackage,
    midi_hotplug::{PanicScope, panic_packets},
};
use rackforge_plugin_api::{PluginKind, PresetCatalog, WebSurfaceKind, abi::MidiEventV1};
use rackforge_repository::install_local_archive;
use rackforge_session_api::{HostControlTarget, MasterLevel, MasterPan};
use rackforge_surface_runtime::{
    ActiveMode, Input as SurfaceInput, Menu as SurfaceMenu, MenuCommand, PlayPlugin, PlaySound,
};
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const SAMPLE_RATE: f64 = 48_000.0;
const MAX_FRAMES: u32 = 4_096;
const MAX_PENDING_MIDI_EVENTS: usize = 256;

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
const HOST_CONTROL_HEADER_MS: u64 = 1_500;

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
    midi: Vec<MidiEventV1>,
    plugin_id: String,
    plugin_name: String,
    plugin_version: String,
    package_root: PathBuf,
    web_entry: String,
    catalog: PresetCatalog,
    selected_sound_id: String,
}

struct SendablePluginInstance(PluginInstance<'static>);

// SAFETY: access to the plugin instance is serialized by ENGINE's mutex. The
// JNI bridge never exposes the instance pointer to Java or another callback.
unsafe impl Send for SendablePluginInstance {}

struct NativeAudioOutput {
    stream: *mut AAudioStream,
}

// SAFETY: the stream is controlled through AAudio's thread-safe lifecycle API
// and is only replaced or dropped while held by AUDIO's mutex.
unsafe impl Send for NativeAudioOutput {}

impl NativeAudioOutput {
    fn open(device_id: i32, latency_mode: i32) -> Result<Self> {
        match latency_mode {
            0 => match open_aaudio_stream(
                device_id,
                AAUDIO_SHARING_MODE_EXCLUSIVE,
                AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
                2,
            ) {
                Ok(stream) => Ok(Self { stream }),
                Err(exclusive_error) => open_aaudio_stream(
                    device_id,
                    AAUDIO_SHARING_MODE_SHARED,
                    AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
                    2,
                )
                .map(|stream| Self { stream })
                .with_context(|| format!("exclusive AAudio open also failed: {exclusive_error:#}")),
            },
            1 => open_aaudio_stream(
                device_id,
                AAUDIO_SHARING_MODE_SHARED,
                AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
                3,
            )
            .map(|stream| Self { stream }),
            2 => open_aaudio_stream(
                device_id,
                AAUDIO_SHARING_MODE_SHARED,
                AAUDIO_PERFORMANCE_MODE_NONE,
                4,
            )
            .map(|stream| Self { stream }),
            _ => bail!("invalid Android latency mode {latency_mode}"),
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
    }
}

impl AndroidEngine {
    fn open(archive: &[u8], store_root: PathBuf, data_root: PathBuf) -> Result<Self> {
        let installed = install_local_archive(&store_root, archive)
            .context("installing the portable plugin")?;
        Self::open_package(&installed.path, data_root)
    }

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
            midi: Vec::with_capacity(256),
            plugin_id,
            plugin_name,
            plugin_version,
            package_root,
            web_entry,
            catalog,
            selected_sound_id,
        })
    }

    fn select_sound(&mut self, sound_id: &str) -> Result<()> {
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
                "config_available": false,
                "sounds": sounds,
                "selected_sound_id": self.selected_sound_id,
            },
            "program_draft": null,
            "audition": null,
            "host": {
                "active_mode": "play",
                "master_level": 0,
                "master_pan": 0,
            }
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
        AAudioStreamBuilder_setDataCallback(builder, Some(render_callback), ptr::null_mut());
        AAudioStreamBuilder_setErrorCallback(builder, Some(error_callback), ptr::null_mut());

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
            "buffer_size_frames": AAudioStream_getBufferSizeInFrames(output.stream),
            "buffer_capacity_frames": AAudioStream_getBufferCapacityInFrames(output.stream),
            "xruns": AAudioStream_getXRunCount(output.stream),
            "sharing_mode": AAudioStream_getSharingMode(output.stream),
            "performance_mode": AAudioStream_getPerformanceMode(output.stream),
            "pending_error": AUDIO_ERROR.load(Ordering::Acquire),
            "midi_dropped_events": MIDI_DROPPED_EVENTS.load(Ordering::Relaxed),
            "midi_panic_count": MIDI_PANIC_COUNT.load(Ordering::Relaxed),
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
    _user_data: *mut c_void,
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
    if let Ok(mut guard) = engine().try_lock()
        && let Some(engine) = guard.as_mut()
        && engine.render(num_frames as u32, output).is_err()
    {
        output.fill(0.0);
    }
    let output_gain = f32::from_bits(OUTPUT_GAIN_BITS.load(Ordering::Relaxed));
    let level_target = f32::from_bits(MASTER_LEVEL_TARGET_BITS.load(Ordering::Relaxed));
    let pan_left_target = f32::from_bits(MASTER_PAN_LEFT_TARGET_BITS.load(Ordering::Relaxed));
    let pan_right_target = f32::from_bits(MASTER_PAN_RIGHT_TARGET_BITS.load(Ordering::Relaxed));
    let mut level = f32::from_bits(MASTER_LEVEL_CURRENT_BITS.load(Ordering::Relaxed));
    let mut pan_left = f32::from_bits(MASTER_PAN_LEFT_CURRENT_BITS.load(Ordering::Relaxed));
    let mut pan_right = f32::from_bits(MASTER_PAN_RIGHT_CURRENT_BITS.load(Ordering::Relaxed));
    for frame in output.chunks_exact_mut(2) {
        smooth_master_sample(&mut level, level_target);
        smooth_master_sample(&mut pan_left, pan_left_target);
        smooth_master_sample(&mut pan_right, pan_right_target);
        frame[0] = (frame[0] * output_gain * level * pan_left).clamp(-1.0, 1.0);
        frame[1] = (frame[1] * output_gain * level * pan_right).clamp(-1.0, 1.0);
    }
    MASTER_LEVEL_CURRENT_BITS.store(level.to_bits(), Ordering::Relaxed);
    MASTER_PAN_LEFT_CURRENT_BITS.store(pan_left.to_bits(), Ordering::Relaxed);
    MASTER_PAN_RIGHT_CURRENT_BITS.store(pan_right.to_bits(), Ordering::Relaxed);
    let elapsed = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    AUDIO_CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
    AUDIO_CALLBACK_FRAMES.fetch_add(num_frames as u64, Ordering::Relaxed);
    AUDIO_CALLBACK_TOTAL_NANOS.fetch_add(elapsed, Ordering::Relaxed);
    AUDIO_CALLBACK_MAX_NANOS.fetch_max(elapsed, Ordering::Relaxed);
    AAUDIO_CALLBACK_RESULT_CONTINUE
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
        "active": active_plugin.is_some_and(|(id, version)| {
            id == &manifest.id && version == &manifest.version
        }),
    })
}

fn installed_plugins_json(store_root: &Path) -> Result<String> {
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
pub extern "system" fn Java_org_rackforge_android_MainActivity_initializeEngine(
    mut env: JNIEnv,
    _class: JClass,
    archive: JByteArray,
    store_root: JString,
    data_root: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let bytes = env.convert_byte_array(&archive)?;
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        let data_root = PathBuf::from(java_string(&mut env, data_root)?);
        let candidate = AndroidEngine::open(&bytes, store_root, data_root)?;
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
        let candidate = NativeAudioOutput::open(device_id, latency_mode)?;
        AUDIO_ERROR.store(AAUDIO_OK, Ordering::Release);
        AUDIO_CALLBACK_COUNT.store(0, Ordering::Relaxed);
        AUDIO_CALLBACK_FRAMES.store(0, Ordering::Relaxed);
        AUDIO_CALLBACK_TOTAL_NANOS.store(0, Ordering::Relaxed);
        AUDIO_CALLBACK_MAX_NANOS.store(0, Ordering::Relaxed);
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
