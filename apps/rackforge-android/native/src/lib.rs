use anyhow::{Context, Result, bail};
use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jstring};
use rackforge_core::{LoadedPlugin, PluginInstance, PluginPackage};
use rackforge_plugin_api::{PluginKind, PresetCatalog, WebSurfaceKind, abi::MidiEventV1};
use rackforge_repository::install_local_archive;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

const SAMPLE_RATE: f64 = 48_000.0;
const MAX_FRAMES: u32 = 4_096;

static ENGINE: OnceLock<Mutex<Option<AndroidEngine>>> = OnceLock::new();
static AUDIO: OnceLock<Mutex<Option<NativeAudioOutput>>> = OnceLock::new();
static MIDI_QUEUE: OnceLock<Mutex<VecDeque<MidiEventV1>>> = OnceLock::new();
static OUTPUT_GAIN_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());

const AAUDIO_OK: i32 = 0;
const AAUDIO_DIRECTION_OUTPUT: i32 = 0;
const AAUDIO_SHARING_MODE_EXCLUSIVE: i32 = 0;
const AAUDIO_SHARING_MODE_SHARED: i32 = 1;
const AAUDIO_FORMAT_PCM_FLOAT: i32 = 2;
const AAUDIO_PERFORMANCE_MODE_NONE: i32 = 10;
const AAUDIO_PERFORMANCE_MODE_LOW_LATENCY: i32 = 12;
const AAUDIO_CALLBACK_RESULT_CONTINUE: i32 = 0;

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
    fn AAudioStreamBuilder_openStream(
        builder: *mut AAudioStreamBuilder,
        stream: *mut *mut AAudioStream,
    ) -> i32;
    fn AAudioStream_requestStart(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_requestStop(stream: *mut AAudioStream) -> i32;
    fn AAudioStream_close(stream: *mut AAudioStream) -> i32;
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
            ) {
                Ok(stream) => Ok(Self { stream }),
                Err(exclusive_error) => open_aaudio_stream(
                    device_id,
                    AAUDIO_SHARING_MODE_SHARED,
                    AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
                )
                .map(|stream| Self { stream })
                .with_context(|| format!("exclusive AAudio open also failed: {exclusive_error:#}")),
            },
            1 => open_aaudio_stream(
                device_id,
                AAUDIO_SHARING_MODE_SHARED,
                AAUDIO_PERFORMANCE_MODE_LOW_LATENCY,
            )
            .map(|stream| Self { stream }),
            2 => open_aaudio_stream(
                device_id,
                AAUDIO_SHARING_MODE_SHARED,
                AAUDIO_PERFORMANCE_MODE_NONE,
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
    MIDI_QUEUE.get_or_init(|| Mutex::new(VecDeque::with_capacity(1024)))
}

fn enqueue_midi(bytes: &[u8]) {
    if bytes.is_empty() || bytes.len() > 3 {
        return;
    }
    let mut data = [0_u8; 3];
    data[..bytes.len()].copy_from_slice(bytes);
    if let Ok(mut queue) = midi_queue().lock() {
        queue.push_back(MidiEventV1 {
            frame: 0,
            length: bytes.len() as u8,
            data,
        });
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

        let mut stream = ptr::null_mut();
        let open_result = AAudioStreamBuilder_openStream(builder, &mut stream);
        let _ = AAudioStreamBuilder_delete(builder);
        if open_result != AAUDIO_OK {
            return Err(aaudio_error("opening AAudio output", open_result));
        }
        if stream.is_null() {
            bail!("AAudio returned a null output stream");
        }
        let start_result = AAudioStream_requestStart(stream);
        if start_result != AAUDIO_OK {
            let _ = AAudioStream_close(stream);
            return Err(aaudio_error("starting AAudio output", start_result));
        }
        Ok(stream)
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
    let gain = f32::from_bits(OUTPUT_GAIN_BITS.load(Ordering::Relaxed));
    if gain != 1.0 {
        for sample in output {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }
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
pub extern "system" fn Java_org_rackforge_android_MainActivity_sendMidi(
    env: JNIEnv,
    _class: JClass,
    message: JByteArray,
) {
    if let Ok(bytes) = env.convert_byte_array(&message) {
        enqueue_midi(&bytes);
    }
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
}
