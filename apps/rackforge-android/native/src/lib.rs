use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use jni::JNIEnv;
use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jstring};
use keylab_essential_mk3::protocol as keylab_protocol;
use rackforge_audio_api::OutputMeter;
use rackforge_control_api::{
    ControlErrorCode, ControlRequest, ControlResponse, ControllerMap, ParameterTouchPickup,
    ParameterTouchReport, PluginParameterControlCommand, PresetImportConflictPolicy,
    RegisteredController, RfPresetFile, parse_plugin_parameter_control_command,
};
use rackforge_core::controller_map_store::{ControllerMapStore, export_rfmap};
use rackforge_core::midi_activity::MidiActivityLog;
use rackforge_core::midi2::Midi2Event;
use rackforge_core::parallel_render::{
    self, ParallelUnits, RenderPool, RenderTelemetry, ScheduledSlot, UnitJob,
    process_slots_sequential, spawn_telemetry_publisher,
};
use rackforge_core::rack_voice::{self, RackEngine};
use rackforge_core::{
    CompiledParameterLink, ControllerMapLinkContext, LiveParameterStateStore, LiveParameterTarget,
    LiveParameterWriter, LiveParameterWriterHandle, LoadedPlugin, PluginInstance, PluginPackage,
    PluginStateStore, PluginStorage, SemanticParameterLinkContext,
    audio_reliability::{
        AudioStreamHealth, AudioStreamRecovery, StereoDropoutRecovery, StereoRenderQueue,
    },
    compile_controller_map_links, compile_semantic_parameter_links,
    isolated_state::{IsolatedPluginStateEditor, validate_state_reference},
    midi_hotplug::{PanicScope, panic_packets},
    performance::PerformanceRepository,
    plugin_parameters, set_plugin_parameter,
};
use rackforge_dsp::output_ceiling;
use rackforge_midi_api::{
    ControlTakeover, IngressMidiEvent, MidiPacket, MidiSourceDescriptor, MidiSourceId,
    MidiSourceKey, MidiSourceRegistry, ParameterLink, ParameterLinkPassThrough,
};
use rackforge_performance_api::{
    LivePerformanceState, PERFORMANCE_SNAPSHOT_SCHEMA_VERSION, PerformanceSnapshot,
};
use rackforge_plugin_api::{
    HostPreset, HostPresetSummary, PROGRAM_EDITOR_SCHEMA_VERSION, PluginKind, PluginStateReference,
    PreparedProgram, PresetCatalog, ProgramDocument, ProgramEditRequest, ProgramEditorValue,
    ProgramEditorView, ProgramFieldEditRequest, WebSurfaceKind,
    abi::{MidiEventV1, ParameterEventV1},
};
use rackforge_repository::{
    PluginUserDataRemovalOptions, cleanup_uninstall_tombstones, inspect_local_archive,
    install_local_archive_cancellable, plugin_is_enabled, remove_plugin_user_data,
    set_plugin_enabled, uninstall_plugin,
};
use rackforge_session_api::{
    ButtonPhase, HostActionBinding, HostControlBinding, HostControlTarget, InstanceId,
    MAX_PLAY_CHAIN_EFFECTS, MasterLevel, MasterPan, PlayChainEffect, ProgramDraftState,
    RackForgeParameterMapper, RackForgeParameterValue, SemanticControlProfile,
    rackforge_parameter_input, semantic_control_input,
};
use rackforge_surface_runtime::{
    ActiveMode, Input as SurfaceInput, Menu as SurfaceMenu, MenuCommand, PlayChainEffectItem,
    PlayPlugin, PlayPreset, PlaySound, ProgramExitDecision, ProgramExitDestination,
    ScreenCompositor, ScreenRegions, SurfaceUpdatePriority,
};
use std::collections::{BTreeMap, VecDeque};
use std::ffi::{CStr, c_char, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const SAMPLE_RATE: f64 = 48_000.0;
const MAX_FRAMES: u32 = 4_096;
const MAX_PENDING_MIDI_EVENTS: usize = 256;
const AUDIO_RENDER_QUEUE_CAPACITY_FRAMES: usize = 2_048;
static PLUGIN_INSTALL_CANCELLED: AtomicBool = AtomicBool::new(false);
const LOW_RENDER_BLOCK_FRAMES: usize = 192;
const BALANCED_RENDER_BLOCK_FRAMES: usize = 384;
const LOW_RENDER_AHEAD_FRAMES: usize = 384;
const BALANCED_RENDER_AHEAD_FRAMES: usize = 1_152;

static ENGINE: OnceLock<Mutex<Option<AndroidEngine>>> = OnceLock::new();
static ISOLATED_PLUGIN_RUNTIMES: OnceLock<Mutex<BTreeMap<String, SendableLoadedPlugin>>> =
    OnceLock::new();
static AUDIO: OnceLock<Mutex<Option<NativeAudioOutput>>> = OnceLock::new();
static OUTPUT_METER: OutputMeter = OutputMeter::new();
static MIDI_QUEUE: OnceLock<Mutex<VecDeque<AndroidMidiIngress>>> = OnceLock::new();
static MIDI_SOURCES: OnceLock<Mutex<MidiSourceRegistry>> = OnceLock::new();
/// Associates Android's runtime-assigned source identity with the
/// `.rfcontroller` package that describes the device behind it.
static MIDI_CONTROLLERS: OnceLock<Mutex<BTreeMap<MidiSourceId, AndroidMidiController>>> =
    OnceLock::new();
/// The player's controller maps, loaded from the data root once Java names it.
static CONTROLLER_MAPS: OnceLock<Mutex<AndroidControllerMaps>> = OnceLock::new();
static MIDI_ACTIVITY: OnceLock<Mutex<MidiActivityLog>> = OnceLock::new();
static CONTROLLER_STORE_ROOT: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static NEXT_MIDI_SOURCE_KEY: AtomicU32 = AtomicU32::new(1);
static CONTROLLER_MENU: OnceLock<Mutex<AndroidControllerMenu>> = OnceLock::new();
static PERFORMANCE: OnceLock<Mutex<Option<AndroidPerformance>>> = OnceLock::new();
static OUTPUT_GAIN_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_LEVEL_VALUE: AtomicU32 = AtomicU32::new(MasterLevel::MAX as u32);
static MASTER_LEVEL_TARGET_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_LEVEL_CURRENT_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_PAN_LEFT_TARGET_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_PAN_LEFT_CURRENT_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_PAN_RIGHT_TARGET_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_PAN_RIGHT_CURRENT_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static MASTER_PAN_VALUE: AtomicI32 = AtomicI32::new(0);
static AUDIO_ERROR: AtomicI32 = AtomicI32::new(AAUDIO_OK);
static MIDI_DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);
static MIDI_PANIC_COUNT: AtomicU64 = AtomicU64::new(0);
static AUDIO_CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);
static AUDIO_CALLBACK_FRAMES: AtomicU64 = AtomicU64::new(0);
static AUDIO_CALLBACK_TOTAL_NANOS: AtomicU64 = AtomicU64::new(0);
static AUDIO_CALLBACK_MAX_NANOS: AtomicU64 = AtomicU64::new(0);
static AUDIO_CALLBACK_OVERRUNS: AtomicU64 = AtomicU64::new(0);
static AUDIO_ENGINE_LOCK_MISSES: AtomicU64 = AtomicU64::new(0);
static AUDIO_RENDER_ERRORS: AtomicU64 = AtomicU64::new(0);
static AUDIO_NONFINITE_SAMPLES: AtomicU64 = AtomicU64::new(0);
static AUDIO_CLIPPED_SAMPLES: AtomicU64 = AtomicU64::new(0);
static AUDIO_RENDER_THREAD_PRIORITY_RESULT: AtomicI32 = AtomicI32::new(0);
static AUDIO_DROPOUT_RECOVERY: StereoDropoutRecovery = StereoDropoutRecovery::new();
static AUDIO_STREAM_RECOVERY: AudioStreamRecovery = AudioStreamRecovery::new();

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
const VIRTUAL_MIDI_SOURCE_KEY: MidiSourceKey = MidiSourceKey::new(u32::MAX);

#[derive(Clone, Copy)]
struct AndroidMidiIngress {
    source: MidiSourceKey,
    event: MidiEventV1,
}

/// One connected MIDI source and the package resolved for it: by the port's
/// name first, and again when the device answers the Identity Request.
struct AndroidMidiController {
    /// The package Java recognised the device as (the KeyLab), or empty.
    hint: String,
    endpoint_name: String,
    identity: Option<rackforge_controller_package::IdentityReply>,
    controller_id: Option<String>,
    identified: bool,
    profile: Option<SemanticControlProfile>,
    /// The package's reserved controls and buttons: the host's, never an
    /// instrument's, as on every other host.
    host_controls: Vec<HostControlBinding>,
    host_actions: Vec<HostActionBinding>,
    on_connect: Vec<Vec<u8>>,
    /// What goes to the device's setup output -- a DAW port -- and how that
    /// port is found among the device's.
    setup_output: Option<rackforge_controller_package::EndpointMatcher>,
    setup_messages: Vec<Vec<u8>>,
    on_connect_sent: bool,
}

impl AndroidMidiController {
    fn new(hint: String, endpoint_name: String) -> Self {
        let mut controller = Self {
            hint,
            endpoint_name,
            identity: None,
            controller_id: None,
            identified: false,
            profile: None,
            host_controls: Vec::new(),
            host_actions: Vec::new(),
            on_connect: Vec::new(),
            setup_output: None,
            setup_messages: Vec::new(),
            on_connect_sent: false,
        };
        controller.resolve();
        controller
    }

    fn resolve(&mut self) {
        let keylab = keylab_essential_mk3::controller::package_profile();
        if keylab.driver_id == self.hint {
            // The in-process driver speaks for the KeyLab, and sends its own
            // messages.
            self.controller_id = Some(keylab.driver_id.clone());
            self.identified = false;
            self.profile = keylab.semantic_profile.clone();
            // The in-process driver reads its own buttons.
            self.host_controls.clear();
            self.host_actions.clear();
            self.on_connect.clear();
            self.setup_output = None;
            self.setup_messages.clear();
            return;
        }
        let binding = controller_store_root().and_then(|root| {
            rackforge_controller_package::PackageStore::new(root)
                .resolve_identified_input(&self.endpoint_name, self.identity.as_ref())
                .map_err(|error| {
                    eprintln!(
                        "ANDROID_DECLARATIVE_CONTROLLER_SKIPPED endpoint={:?} error={error}",
                        self.endpoint_name
                    );
                })
                .ok()
                .flatten()
        });
        self.controller_id = binding
            .as_ref()
            .map(|binding| binding.controller_id.clone());
        self.identified = binding.as_ref().is_some_and(|binding| binding.identified);
        let previous = (self.on_connect.clone(), self.setup_messages.clone());
        self.on_connect = binding
            .as_ref()
            .map(|binding| binding.on_connect.clone())
            .unwrap_or_default();
        self.host_controls = binding
            .as_ref()
            .map(|binding| binding.host_controls.clone())
            .unwrap_or_default();
        self.host_actions = binding
            .as_ref()
            .map(|binding| binding.host_actions.clone())
            .unwrap_or_default();
        self.setup_output = binding
            .as_ref()
            .and_then(|binding| binding.setup_output.clone());
        self.setup_messages = binding
            .as_ref()
            .map(|binding| binding.setup_messages.clone())
            .unwrap_or_default();
        // An Identity Reply that chose another package, or an output the
        // player just allowed: its messages have not gone out yet.
        if (self.on_connect.clone(), self.setup_messages.clone()) != previous {
            self.on_connect_sent = false;
        }
        self.profile = binding.and_then(|binding| binding.semantic_profile);
    }
}

#[derive(Default)]
struct AndroidControllerMaps {
    store: Option<ControllerMapStore>,
    data_root: Option<PathBuf>,
    /// Where the plugins and the controller packages are installed: the
    /// maps offered are made from what is there.
    plugin_store_root: Option<PathBuf>,
    controller_store_root: Option<PathBuf>,
    maps: BTreeMap<String, ControllerMap>,
    /// How knobs and faders take a parameter over, as stored.
    takeover: ControlTakeover,
}

#[derive(Default)]
struct AndroidControllerMenu {
    menu: SurfaceMenu,
    compositor: ScreenCompositor,
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

impl AndroidControllerMenu {
    fn render_plan(
        &mut self,
        priority: SurfaceUpdatePriority,
    ) -> Result<Vec<keylab_protocol::OutboundMessage>> {
        self.compositor.publish(self.menu.render(), priority);
        self.compositor
            .take()
            .map_or_else(
                || Ok(Vec::new()),
                |update| keylab_protocol::render_update_messages(&update),
            )
            .map_err(anyhow::Error::msg)
    }

    fn render_response(&mut self, command: Option<serde_json::Value>) -> Result<String> {
        Ok(serde_json::json!({
            "plan": controller_plan_value(Ok(self.render_plan(SurfaceUpdatePriority::Interactive)?))?,
            "command": command,
        })
        .to_string())
    }

    fn render_rackforge_parameter(&mut self, parameter: RackForgeParameterValue) -> Result<String> {
        self.render_transient_header(&parameter.little_header())
    }

    /// A header shown for a moment over the screen -- what a control just
    /// did -- after which Java restores the screen.
    fn render_transient_header(&mut self, header: &str) -> Result<String> {
        self.compositor
            .publish(self.menu.render(), SurfaceUpdatePriority::Background);
        self.compositor
            .invalidate(ScreenRegions::header(), SurfaceUpdatePriority::Immediate);
        Ok(serde_json::json!({
            "plan": controller_plan_value(keylab_protocol::transient_header_messages(header))?,
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
        let command = command.and_then(|command| self.dispatch(command));
        while self.menu.take_command().is_some() {}
        command
    }

    /// Commands the in-process host satisfies natively, without a round trip
    /// through Java: the performance library (the SAME file-backed repository
    /// the Pi core uses) and the plugin program editor (the machinery the web
    /// panel already drives). Everything else keeps the JSON contract with
    /// MainActivity.
    fn dispatch(&mut self, command: MenuCommand) -> Option<serde_json::Value> {
        match command {
            MenuCommand::EditPerformance {
                expected_revision,
                edit,
            } => {
                let result = (|| -> Result<PerformanceSnapshot> {
                    let mut guard = performance()
                        .lock()
                        .map_err(|_| anyhow::anyhow!("performance library lock poisoned"))?;
                    let state = guard
                        .as_mut()
                        .context("the performance library is not ready yet")?;
                    let mut live = state.live.clone();
                    state
                        .repository
                        .apply_edit(&expected_revision, edit, &mut live)?;
                    state.live = live;
                    Ok(performance_snapshot(state))
                })();
                self.menu
                    .complete_performance_edit(result.map_err(|error| format!("{error:#}")));
                None
            }
            MenuCommand::SetLiveBrowseMode { mode } => {
                if let Ok(mut guard) = performance().lock()
                    && let Some(state) = guard.as_mut()
                {
                    state.live.mode = mode;
                    let snapshot = performance_snapshot(state);
                    drop(guard);
                    self.menu.sync_performance_snapshot(snapshot);
                }
                None
            }
            // The web and LITTLE load a Rack the same way: through the
            // Activity, which switches the one voice to the Rack's instrument
            // with its saved sound and commits the location once it sounds.
            MenuCommand::ActivateLiveTarget { location } => Some(serde_json::json!({
                "type": "activate_live",
                "location": location,
            })),
            // Previewing a DRAFT Rack needs a rack engine Android does not
            // have yet; saving and activating are the honest paths.
            MenuCommand::PreviewRack { rack } => Some(serde_json::json!({
                "type": "preview_rack",
                "rack": rack,
            })),
            MenuCommand::LoadPluginPreset { preset_id } => {
                let result = engine_call(|engine| {
                    engine.plugin_state_command(
                        "load_preset",
                        &serde_json::json!({ "preset_id": &preset_id }),
                    )?;
                    Ok(())
                })
                .and_then(|()| sync_menu_program_state(&mut self.menu));
                match result {
                    Ok(()) => {
                        self.menu.complete_plugin_preset_load(&preset_id);
                    }
                    Err(error) => eprintln!("PLUGIN_PRESET_LOAD_FAILED {error:#}"),
                }
                None
            }
            MenuCommand::SavePluginPreset { name } => {
                let result = engine_call(|engine| {
                    let preset: HostPreset = serde_json::from_value(engine.plugin_state_command(
                        "save_preset",
                        &serde_json::json!({ "name": &name }),
                    )?)?;
                    let presets: Vec<HostPresetSummary> = serde_json::from_value(
                        engine.plugin_state_command("list_presets", &serde_json::json!({}))?,
                    )?;
                    Ok((preset, presets))
                })
                .map(|(preset, presets)| {
                    let saved = PlayPreset::new(
                        preset.id.clone(),
                        preset.name.clone(),
                        format!("v{}", preset.state.plugin_version),
                    );
                    let presets = presets
                        .into_iter()
                        .map(|preset| {
                            PlayPreset::new(
                                preset.id,
                                preset.name,
                                format!("v{}", preset.plugin_version),
                            )
                        })
                        .collect();
                    (saved, presets)
                })
                .map_err(|error| format!("{error:#}"));
                if let Err(error) = &result {
                    eprintln!("PLUGIN_PRESET_SAVE_FAILED {error}");
                }
                self.menu.complete_plugin_preset_save(result);
                None
            }
            MenuCommand::SetPluginParameter {
                instance_id,
                parameter_index,
                value,
            } => {
                match set_android_little_parameter(&instance_id, parameter_index, value) {
                    Ok(value) => self
                        .menu
                        .complete_plugin_parameter_set(parameter_index, value),
                    Err(error) => eprintln!("LITTLE_PARAMETER_SET_FAILED {error:#}"),
                }
                None
            }
            MenuCommand::TriggerPluginParameter {
                instance_id,
                parameter_index,
            } => {
                let result = set_android_little_parameter(&instance_id, parameter_index, 1.0)
                    .and_then(|_| set_android_little_parameter(&instance_id, parameter_index, 0.0));
                match result {
                    Ok(value) => self
                        .menu
                        .complete_plugin_parameter_set(parameter_index, value),
                    Err(error) => eprintln!("LITTLE_PARAMETER_TRIGGER_FAILED {error:#}"),
                }
                None
            }
            MenuCommand::BeginProgramEdit { program_id } => {
                let result = engine_call(|engine| engine.begin_program_edit(program_id))
                    .and_then(|()| sync_menu_program_state(&mut self.menu));
                if let Err(error) = result {
                    eprintln!("PROGRAM_EDIT_START_FAILED {error:#}");
                    self.menu.sync_program_edit(None, None);
                }
                None
            }
            MenuCommand::EditProgramDraftField {
                draft_id,
                field_id,
                value,
                preview,
            } => {
                let result = engine_call(|engine| {
                    engine.edit_program_field(draft_id, field_id, value, preview)
                })
                .and_then(|()| {
                    if preview {
                        Ok(())
                    } else {
                        sync_menu_program_state(&mut self.menu)
                    }
                });
                if let Err(error) = result {
                    eprintln!("PROGRAM_DRAFT_FIELD_FAILED {error:#}");
                }
                None
            }
            MenuCommand::RestoreProgramDraftPreview { draft_id } => {
                if let Err(error) = engine_call(|engine| engine.restore_program_preview(draft_id)) {
                    eprintln!("PROGRAM_DRAFT_RESTORE_FAILED {error:#}");
                }
                None
            }
            MenuCommand::SetProgramDraftName { draft_id, name } => {
                let result = engine_call(|engine| engine.set_program_name(draft_id, name))
                    .and_then(|()| sync_menu_program_state(&mut self.menu));
                if let Err(error) = result {
                    eprintln!("PROGRAM_DRAFT_NAME_FAILED {error:#}");
                }
                None
            }
            MenuCommand::SaveProgramDraft { draft_id } => {
                let result = engine_call(|engine| engine.save_program(draft_id))
                    .and_then(|()| sync_menu_program_state(&mut self.menu));
                if let Err(error) = result {
                    eprintln!("PROGRAM_SAVE_FAILED {error:#}");
                }
                None
            }
            MenuCommand::CancelProgramEdit { draft_id } => {
                let result = engine_call(|engine| engine.cancel_program_edit(draft_id))
                    .and_then(|()| sync_menu_program_state(&mut self.menu));
                if let Err(error) = result {
                    eprintln!("PROGRAM_CANCEL_FAILED {error:#}");
                }
                None
            }
            MenuCommand::ResolveProgramExit {
                draft_id,
                decision,
                destination,
            } => {
                let result = engine_call(|engine| match decision {
                    ProgramExitDecision::Save => engine.save_program(draft_id),
                    ProgramExitDecision::Discard => engine.cancel_program_edit(draft_id),
                })
                .and_then(|()| sync_menu_program_state(&mut self.menu));
                if let Err(error) = result {
                    eprintln!("PROGRAM_EXIT_FAILED {error:#}");
                }
                if let ProgramExitDestination::ActiveMode {
                    mode,
                    selected_sound_id,
                } = destination
                {
                    self.menu
                        .complete_return_to_active_mode(mode, selected_sound_id.as_deref());
                    return Some(serde_json::json!({
                        "type": "return_mode",
                        "mode": active_mode_name(mode),
                        "sound_id": selected_sound_id,
                    }));
                }
                None
            }
            other => self.command_json(other),
        }
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
            // The chain belongs to the instrument and is kept by the Activity
            // beside the rest of the session, so LITTLE's edits take the same
            // road as its plugin and program edits.
            MenuCommand::AddPlayChainEffect { plugin_id } => Some(serde_json::json!({
                "type": "play_chain_add",
                "plugin_id": plugin_id,
            })),
            MenuCommand::SetPlayChainEffectEnabled { effect_id, enabled } => {
                Some(serde_json::json!({
                    "type": "play_chain_enabled",
                    "effect_id": effect_id,
                    "enabled": enabled,
                }))
            }
            MenuCommand::RemovePlayChainEffect { effect_id } => Some(serde_json::json!({
                "type": "play_chain_remove",
                "effect_id": effect_id,
            })),
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
    fn AAudioStream_getDeviceId(stream: *mut AAudioStream) -> i32;
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
    /// Host-owned unit instances for `parallel_render_v1` plugins, so the
    /// Android PLAY voice schedules units across the shared worker pool.
    parallel: Option<SendableParallelUnits>,
    process_faulted: bool,
    render_pool: RenderPool,
    render_telemetry: std::sync::Arc<RenderTelemetry>,
    runtime: SendableLoadedPlugin,
    /// The effects after the instrument, in the order they play. PLAY's
    /// audio is the instrument, then every effect that is on, then output.
    chain: Vec<ChainVoice>,
    /// What the stage before an effect wrote, so an effect never reads the
    /// block it is writing.
    chain_scratch: Vec<f32>,
    midi: Vec<MidiEventV1>,
    /// The same block's MIDI in the host's vocabulary, for the pool's
    /// pre-stage, which cuts it by the families the plugin declared wide.
    wide: Vec<Midi2Event>,
    parameter_events: Vec<ParameterEventV1>,
    parameter_links: Vec<CompiledParameterLink>,
    persisted_parameter_links: Vec<ParameterLink>,
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
    _live_parameter_writer: LiveParameterWriter,
    live_parameter_writer_handle: LiveParameterWriterHandle,
    /// The LIVE Rack, while one is loaded. It plays in place of PLAY's
    /// voice and chain, which stay built and untouched for the way back.
    rack: Option<AndroidRack>,
}

/// A LIVE Rack on Android: every Slot, through the same Rack code the
/// appliance plays it with.
struct AndroidRack {
    rack_id: String,
    engine: RackEngine<'static>,
}

// SAFETY: Android admits only portable wasm-v1 plugins, so every Slot's
// instance and unit instances live in the sandbox. The Rack is owned by the
// engine behind ENGINE's mutex, and the render pool enters a Slot only under
// its epoch protocol, as it does the PLAY voice.
unsafe impl Send for AndroidRack {}

/// Set by a panic: the next block resets every Slot of a loaded Rack. The
/// panic's all-notes-off messages also go through the queue, but a Slot
/// whose stages take notes only would never hear them.
static RACK_RESET_PENDING: AtomicBool = AtomicBool::new(false);

struct SendablePluginInstance(PluginInstance<'static>);

struct SendableParallelUnits(ParallelUnits<'static>);

// SAFETY: unit instances are portable wasm-v1 sandboxes created next to the
// coordinator; ownership stays with the engine behind its mutex and each
// unit is entered by at most one pool worker per block under the epoch
// protocol.
unsafe impl Send for SendableParallelUnits {}

/// One effect of PLAY's chain: its own instance of the plugin, on its own
/// program, addressed by a panel as `android-main.fx.<effect id>`.
///
/// The same plugin may sit in a chain twice; each is a voice of its own, and
/// what the chain remembers of it is its program.
struct ChainVoice {
    effect_id: String,
    instance_id: String,
    plugin_id: String,
    plugin_name: String,
    plugin_short_name: String,
    plugin_version: String,
    config_available: bool,
    enabled: bool,
    program_id: Option<String>,
    input_channels: u32,
    catalog: PresetCatalog,
    runtime: SendableLoadedPlugin,
    instance: SendablePluginInstance,
    /// A stage that faulted is passed over rather than played through, the
    /// way the instrument's own quarantine silences it.
    faulted: bool,
}

/// The engine's single PLAY voice, borrowed for one block so the shared
/// worker pool can schedule its units. Android has no Rack engine, so this
/// one-Slot graph is the whole render.
struct AndroidSlot<'block> {
    instance: &'block mut PluginInstance<'static>,
    parallel: Option<&'block mut ParallelUnits<'static>>,
    output: &'block mut [f32],
    midi: &'block [MidiEventV1],
    wide: &'block [Midi2Event],
    parameter_events: &'block [ParameterEventV1],
    faulted: &'block mut bool,
}

// SAFETY: the same argument as the other hosts — the coordinator and every
// unit instance run in the portable backend, and unit jobs point at
// per-unit boxed cells owned by `ParallelUnits`.
unsafe impl ScheduledSlot for AndroidSlot<'_> {
    fn max_units(&self) -> u32 {
        self.parallel
            .as_ref()
            .map_or(0, |parallel| parallel.max_units())
    }

    fn run_single(&mut self, frames: u32, channels: u32) -> bool {
        self.output.fill(0.0);
        if *self.faulted {
            return true;
        }
        self.instance
            .process_interleaved(
                &[],
                self.output,
                frames,
                0,
                channels,
                self.midi,
                self.parameter_events,
            )
            .is_ok()
    }

    fn run_begin(&mut self, frames: u32, _channels: u32) -> Option<u32> {
        self.output.fill(0.0);
        if *self.faulted {
            return Some(0);
        }
        let parallel = self.parallel.as_mut()?;
        parallel
            .begin(self.instance, &[], frames, self.wide, self.parameter_events)
            .ok()
    }

    fn unit_job(&mut self, unit: u32, frames: u32, channels: u32) -> UnitJob {
        self.parallel
            .as_mut()
            .expect("unit job requested for a classic Android voice")
            .unit_job(unit, &[], frames, channels)
    }

    fn run_end(&mut self, frames: u32, channels: u32, completed: u32) -> bool {
        if *self.faulted {
            return true;
        }
        let Some(parallel) = self.parallel.as_mut() else {
            return false;
        };
        parallel
            .finish(self.instance, self.output, frames, channels, completed)
            .is_ok()
    }

    fn quarantine(&mut self) {
        self.output.fill(0.0);
        *self.faulted = true;
    }
}

#[derive(Clone, Copy)]
struct SendableLoadedPlugin(&'static LoadedPlugin);

#[derive(Clone)]
struct AndroidIsolatedStateContext {
    /// The active plugin's runtime, when a plugin is active: a Slot holding
    /// that plugin shares it. Any other plugin is loaded from its package.
    runtime: Option<SendableLoadedPlugin>,
    resource_overrides: BTreeMap<String, PathBuf>,
    data_root: PathBuf,
}

impl AndroidIsolatedStateContext {
    /// No plugin is active: every Slot's plugin comes from its package.
    fn without_engine(data_root: &Path) -> Self {
        Self {
            runtime: None,
            resource_overrides: BTreeMap::new(),
            data_root: data_root.to_path_buf(),
        }
    }

    /// The active plugin's runtime, when it is the one asked for.
    fn active(&self, plugin_id: &str, version: Option<&str>) -> Option<SendableLoadedPlugin> {
        self.runtime.filter(|runtime| {
            let manifest = runtime.0.manifest();
            manifest.id == plugin_id && version.is_none_or(|version| manifest.version == version)
        })
    }
}

// SAFETY: access to the plugin instance is serialized by ENGINE's mutex. The
// JNI bridge never exposes the instance pointer to Java or another callback.
unsafe impl Send for SendablePluginInstance {}

// SAFETY: AndroidEngine accepts only validated portable wasm-v1 packages.
// Their LoadedPlugin backend is immutable compiled Wasm state; native plugin
// host pointers can never inhabit this wrapper on Android.
unsafe impl Send for SendableLoadedPlugin {}
unsafe impl Sync for SendableLoadedPlugin {}

struct AudioRenderWorker {
    queue: Arc<StereoRenderQueue>,
    stop: Arc<AtomicBool>,
    thread: thread::Thread,
    handle: Option<JoinHandle<()>>,
}

impl AudioRenderWorker {
    fn start(block_frames: usize, render_ahead_frames: usize) -> Result<Self> {
        let queue = Arc::new(StereoRenderQueue::new(AUDIO_RENDER_QUEUE_CAPACITY_FRAMES)?);
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
    queue: Arc<StereoRenderQueue>,
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
            bail!("PLAY's stage takes an instrument; an effect belongs to its chain");
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
        let live_parameter_store = LiveParameterStateStore::open(Some(&data_root))?;
        instance.activate(SAMPLE_RATE, MAX_FRAMES, 0, 2)?;
        let live_parameter_writer = LiveParameterWriter::start(
            live_parameter_store,
            vec![LiveParameterTarget {
                plugin_id: plugin_id.clone(),
                plugin_version: plugin_version.clone(),
                schema: plugin.parameters().clone(),
            }],
        );
        let live_parameter_writer_handle = live_parameter_writer.handle();
        let parallel = Self::rebuild_parallel_units(plugin, &BTreeMap::new(), &selected_sound_id);
        let render_telemetry = RenderTelemetry::new(parallel_render::MAX_RENDER_SLOTS);
        spawn_telemetry_publisher(&render_telemetry, std::time::Duration::from_secs(1));
        let render_pool = RenderPool::automatic(std::sync::Arc::clone(&render_telemetry));
        Ok(Self {
            instance: SendablePluginInstance(instance),
            parallel,
            process_faulted: false,
            render_pool,
            render_telemetry,
            runtime: SendableLoadedPlugin(plugin),
            chain: Vec::new(),
            chain_scratch: Vec::new(),
            midi: Vec::with_capacity(256),
            wide: Vec::with_capacity(256),
            parameter_events: Vec::with_capacity(256),
            parameter_links: Vec::new(),
            persisted_parameter_links: Vec::new(),
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
            _live_parameter_writer: live_parameter_writer,
            live_parameter_writer_handle,
            rack: None,
        })
    }

    /// Replaces the effects after the instrument.
    ///
    /// A stage whose id and plugin are unchanged keeps playing, and its tail
    /// with it: only what the player actually changed is rebuilt.
    fn set_play_chain(&mut self, effects: &[PlayChainEffect], store_root: &Path) -> Result<()> {
        if effects.len() > MAX_PLAY_CHAIN_EFFECTS {
            bail!("a PLAY chain holds at most {MAX_PLAY_CHAIN_EFFECTS} effects");
        }
        let mut previous = std::mem::take(&mut self.chain);
        let mut built = Vec::with_capacity(effects.len());
        for effect in effects {
            if let Some(position) = previous.iter().position(|voice| {
                voice.effect_id == effect.id && voice.plugin_id == effect.plugin_id
            }) {
                let mut voice = previous.remove(position);
                voice.enabled = effect.enabled;
                if voice.program_id != effect.program_id
                    && let Some(program) = effect.program_id.as_deref()
                    && let Err(error) = voice.instance.0.load_preset(program)
                {
                    eprintln!(
                        "PLAY_CHAIN_PROGRAM_SKIPPED effect={} program={program} reason={error:#}",
                        effect.id
                    );
                }
                voice.program_id = effect.program_id.clone();
                built.push(voice);
                continue;
            }
            match Self::build_chain_voice(effect, store_root, &self.data_root) {
                Ok(voice) => built.push(voice),
                Err(error) => {
                    // A chain kept from a session where the effect was
                    // installed and this one where it is not: the rest of the
                    // chain still plays.
                    eprintln!(
                        "PLAY_CHAIN_EFFECT_SKIPPED effect={} plugin={} reason={error:#}",
                        effect.id, effect.plugin_id
                    );
                }
            }
        }
        self.chain = built;
        Ok(())
    }

    /// Builds one effect's voice: its package from the store, its program if
    /// the chain named one, activated for the block the instrument plays.
    fn build_chain_voice(
        effect: &PlayChainEffect,
        store_root: &Path,
        data_root: &Path,
    ) -> Result<ChainVoice> {
        let instance_id = InstanceId::new(format!("{ANDROID_INSTANCE_ID}.fx.{}", effect.id))
            .map_err(anyhow::Error::msg)?
            .as_str()
            .to_owned();
        let package = installed_package(store_root, &effect.plugin_id)?;
        let manifest = package.manifest();
        if manifest.kind != PluginKind::Effect {
            bail!("{} is not an effect", effect.plugin_id);
        }
        manifest
            .portable_component()
            .context("Android requires a portable wasm-v1 plugin")?;
        let audio = manifest.resolved_audio_contract();
        let input_channels = audio.input_channels();
        if input_channels != 2 {
            bail!("PLAY's chain is stereo and this effect takes {input_channels} channels");
        }
        let plugin_name = manifest.name.clone();
        let plugin_short_name = manifest.little_short_name();
        let plugin_version = manifest.version.clone();
        let config_available = manifest.web_ui.as_ref().is_some_and(|web| {
            web.surfaces
                .iter()
                .any(|surface| surface.kind == WebSurfaceKind::Config)
        });
        let runtime = cached_isolated_plugin_runtime(&package, data_root)?;
        let mut instance = runtime.0.create_instance()?;
        let catalog = instance.preset_catalog()?;
        let mut program_id = effect.program_id.clone();
        if let Some(program) = program_id.clone()
            && let Err(error) = instance.load_preset(&program)
        {
            eprintln!(
                "PLAY_CHAIN_PROGRAM_SKIPPED effect={} program={program} reason={error:#}",
                effect.id
            );
            program_id = None;
        }
        instance
            .activate(SAMPLE_RATE, MAX_FRAMES, input_channels, 2)
            .with_context(|| format!("activating {}", effect.plugin_id))?;
        Ok(ChainVoice {
            effect_id: effect.id.clone(),
            instance_id,
            plugin_id: effect.plugin_id.clone(),
            plugin_name,
            plugin_short_name,
            plugin_version,
            config_available,
            enabled: effect.enabled,
            program_id,
            input_channels,
            catalog,
            runtime,
            instance: SendablePluginInstance(instance),
            faulted: false,
        })
    }

    /// The chain over one block: each effect reads what the stage before it
    /// wrote. Effects take no MIDI -- they are not played, they are played
    /// through.
    fn render_chain(&mut self, frames: u32, output: &mut [f32]) {
        let samples = frames as usize * 2;
        let scratch = &mut self.chain_scratch;
        for voice in &mut self.chain {
            if !voice.enabled || voice.faulted {
                continue;
            }
            if scratch.len() < samples {
                scratch.resize(samples, 0.0);
            }
            scratch[..samples].copy_from_slice(&output[..samples]);
            let processed = voice.instance.0.process_interleaved(
                &scratch[..samples],
                &mut output[..samples],
                frames,
                voice.input_channels,
                2,
                &[],
                &[],
            );
            if processed.is_err() {
                output[..samples].copy_from_slice(&scratch[..samples]);
                voice.faulted = true;
                eprintln!(
                    "PLUGIN_PROCESS_QUARANTINED context=android:{} action=bypass",
                    voice.plugin_id
                );
            }
        }
    }

    /// An effect of the chain takes its program in its own voice, and the
    /// chain remembers which: the same effect twice is two settings.
    fn select_chain_sound(&mut self, instance_id: &str, sound_id: &str) -> Result<()> {
        let voice = self
            .chain
            .iter_mut()
            .find(|voice| voice.instance_id == instance_id)
            .with_context(|| format!("effect {instance_id} is not in the PLAY chain"))?;
        if !voice
            .catalog
            .presets
            .iter()
            .any(|preset| preset.id == sound_id)
        {
            bail!("effect does not expose program {sound_id:?}");
        }
        voice.instance.0.load_preset(sound_id)?;
        voice.program_id = Some(sound_id.to_owned());
        voice.faulted = false;
        Ok(())
    }

    /// The chain as LITTLE lists it: what is playing, in order, each under
    /// the instance its panel addresses.
    fn little_chain_items(&self) -> Vec<PlayChainEffectItem> {
        self.chain
            .iter()
            .map(|voice| PlayChainEffectItem {
                id: voice.effect_id.clone(),
                instance_id: voice.instance_id.clone(),
                plugin_id: voice.plugin_id.clone(),
                name: voice.plugin_name.clone(),
                short_name: voice.plugin_short_name.clone(),
                enabled: voice.enabled,
            })
            .collect()
    }

    /// The chain as the session sees it, with one instance for each effect
    /// so a panel can open on it and read its programs.
    fn play_chain_json(&self) -> serde_json::Value {
        let effects = self
            .chain
            .iter()
            .map(|voice| {
                serde_json::json!({
                    "id": voice.effect_id,
                    "plugin_id": voice.plugin_id,
                    "enabled": voice.enabled,
                    "program_id": voice.program_id,
                })
            })
            .collect::<Vec<_>>();
        let instances = self
            .chain
            .iter()
            .map(|voice| {
                let sounds = voice
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
                let selected = voice.program_id.clone().or_else(|| {
                    voice
                        .catalog
                        .presets
                        .first()
                        .map(|preset| preset.id.clone())
                });
                serde_json::json!({
                    "instance_id": voice.instance_id,
                    "plugin_id": voice.plugin_id,
                    "plugin_name": voice.plugin_name,
                    "plugin_short_name": voice.plugin_short_name,
                    "plugin_version": voice.plugin_version,
                    "ui_layouts": ["little@1"],
                    "config_available": voice.config_available,
                    "effect": true,
                    "sounds": sounds,
                    "selected_sound_id": selected,
                })
            })
            .collect::<Vec<_>>();
        serde_json::json!({"effects": effects, "instances": instances})
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
        self.mirror_control(|instance| instance.load_preset(sound_id))?;
        self.process_faulted = false;
        self.live_parameter_writer_handle.clear(0);
        self.selected_sound_id = sound_id.to_owned();
        Ok(())
    }

    /// Applies one control-plane operation to the coordinator and mirrors
    /// the identical canonical input to every unit instance.
    fn mirror_control<E>(
        &mut self,
        mut operation: impl FnMut(&mut PluginInstance<'static>) -> std::result::Result<(), E>,
    ) -> std::result::Result<(), E> {
        operation(&mut self.instance.0)?;
        if let Some(parallel) = self.parallel.as_mut() {
            parallel.0.mirror(operation)?;
        }
        Ok(())
    }

    /// Rebuilds the unit family around a replacement coordinator, mirroring
    /// the same canonical inputs it just received. Control threads only.
    fn rebuild_parallel_units(
        plugin: &'static LoadedPlugin,
        resource_overrides: &BTreeMap<String, PathBuf>,
        sound_id: &str,
    ) -> Option<SendableParallelUnits> {
        if !parallel_render::parallel_units_enabled() {
            return None;
        }
        let mut units = match ParallelUnits::create_with_resources(
            plugin,
            resource_overrides,
            SAMPLE_RATE,
            MAX_FRAMES,
            0,
            2,
        ) {
            Ok(Some(units)) => units,
            Ok(None) => return None,
            Err(error) => {
                eprintln!("ANDROID_PLAY_UNITS_SKIPPED error={error:#}");
                return None;
            }
        };
        if let Err(error) = units.mirror(|instance| instance.load_preset(sound_id)) {
            eprintln!("ANDROID_PLAY_UNITS_SKIPPED error={error:#}");
            return None;
        }
        Some(SendableParallelUnits(units))
    }

    fn restore_sound(&mut self, sound_id: &str) -> Result<()> {
        if !self
            .catalog
            .presets
            .iter()
            .any(|preset| preset.id == sound_id)
        {
            bail!("plugin does not expose sound {sound_id:?}");
        }
        self.mirror_control(|instance| instance.load_preset(sound_id))?;
        self.process_faulted = false;
        self.live_parameter_writer_handle.flush();
        let store = LiveParameterStateStore::open(Some(&self.data_root))?;
        for (parameter_index, value) in
            store.restored_values(&self.plugin_id, self.runtime.0.parameters())
        {
            // A value the plugin refuses costs that one parameter, never the
            // sound.
            if let Err(error) =
                set_plugin_parameter(self.runtime.0, &mut self.instance.0, parameter_index, value)
            {
                eprintln!(
                    "LIVE_PARAMETER_NOT_RESTORED plugin={} parameter={parameter_index} value={value} error={error:#}",
                    self.plugin_id
                );
                continue;
            }
            if let Some(parallel) = self.parallel.as_mut() {
                parallel
                    .0
                    .mirror(|instance| instance.set_parameter(parameter_index, value))?;
            }
        }
        self.selected_sound_id = sound_id.to_owned();
        Ok(())
    }

    fn plugin_parameter_command(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // An effect's panel addresses the effect's own voice, whose schema is
        // its plugin's. The instrument's own path is below.
        if let Some(instance_id) = params.get("instance_id").and_then(|value| value.as_str())
            && let Some(position) = self
                .chain
                .iter()
                .position(|voice| voice.instance_id == instance_id)
        {
            return self.chain_parameter_command(method, params, position);
        }
        let response =
            match parse_plugin_parameter_control_command(method, ANDROID_INSTANCE_ID, params)
                .map_err(anyhow::Error::msg)?
            {
                PluginParameterControlCommand::Read { instance_id } => {
                    let (schema, values) = plugin_parameters(self.runtime.0, &mut self.instance.0)?;
                    ControlResponse::PluginParameters {
                        instance_id,
                        schema: Box::new(schema),
                        values,
                    }
                }
                PluginParameterControlCommand::Set {
                    instance_id,
                    parameter_index,
                    value,
                } => {
                    let requested = value;
                    let value = set_plugin_parameter(
                        self.runtime.0,
                        &mut self.instance.0,
                        parameter_index,
                        requested,
                    )?;
                    if let Some(parallel) = self.parallel.as_mut() {
                        parallel.0.mirror(|instance| {
                            instance.set_parameter(parameter_index, requested)
                        })?;
                    }
                    self.live_parameter_writer_handle
                        .try_record(0, parameter_index, value);
                    ControlResponse::PluginParameterSet {
                        instance_id,
                        parameter_index,
                        value,
                    }
                }
            };
        serde_json::to_value(response).context("serializing plugin parameter response")
    }

    /// One parameter read or write on an effect of the chain. Its values
    /// belong to the voice and are not persisted with the instrument's: a
    /// chain carries its settings in its own programs.
    fn chain_parameter_command(
        &mut self,
        method: &str,
        params: &serde_json::Value,
        position: usize,
    ) -> Result<serde_json::Value> {
        let expected = self.chain[position].instance_id.clone();
        let runtime = self.chain[position].runtime;
        let voice = &mut self.chain[position];
        let response = match parse_plugin_parameter_control_command(method, &expected, params)
            .map_err(anyhow::Error::msg)?
        {
            PluginParameterControlCommand::Read { instance_id } => {
                let (schema, values) = plugin_parameters(runtime.0, &mut voice.instance.0)?;
                ControlResponse::PluginParameters {
                    instance_id,
                    schema: Box::new(schema),
                    values,
                }
            }
            PluginParameterControlCommand::Set {
                instance_id,
                parameter_index,
                value,
            } => {
                let value =
                    set_plugin_parameter(runtime.0, &mut voice.instance.0, parameter_index, value)?;
                ControlResponse::PluginParameterSet {
                    instance_id,
                    parameter_index,
                    value,
                }
            }
        };
        serde_json::to_value(response).context("serializing plugin parameter response")
    }

    fn plugin_state_command(
        &mut self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        if matches!(
            method,
            "materialize" | "catalog" | "plugin_state_parameters" | "set_plugin_state_parameter"
        ) {
            return isolated_plugin_state_command(&self.isolated_state_context(), method, params);
        }
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
            "save_preset" => {
                let name = params
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .context("save preset command is missing name")?;
                let bytes = self.instance.0.save_state()?;
                let state = store.put(
                    &self.plugin_id,
                    &self.plugin_version,
                    self.runtime.0.manifest().state_version,
                    Some(self.selected_sound_id.clone()),
                    &bytes,
                )?;
                Ok(serde_json::to_value(store.save_preset(name, state)?)?)
            }
            "load_preset" => {
                let preset_id = params
                    .get("preset_id")
                    .and_then(serde_json::Value::as_str)
                    .context("load preset command is missing preset_id")?;
                let preset = store.preset(&self.plugin_id, preset_id)?;
                let installed_state_version = self.runtime.0.manifest().state_version;
                if preset.state.state_version != installed_state_version {
                    bail!(
                        "preset state v{} is incompatible with installed state v{}",
                        preset.state.state_version,
                        installed_state_version
                    );
                }
                let bytes = store.read(&preset.state)?;
                self.mirror_control(|instance| instance.load_state(&bytes))?;
                self.process_faulted = false;
                self.live_parameter_writer_handle.clear(0);
                if let Some(sound_id) = preset.state.selected_sound_id.as_ref() {
                    self.selected_sound_id = sound_id.clone();
                }
                Ok(serde_json::to_value(preset)?)
            }
            // A Rack Slot's saved sound, loaded into the one voice Android
            // plays: LIVE loads a Rack this way.
            "rename_preset" => {
                let preset_id = params
                    .get("preset_id")
                    .and_then(serde_json::Value::as_str)
                    .context("rename preset command is missing preset_id")?;
                let name = params
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .context("rename preset command is missing name")?;
                Ok(serde_json::to_value(store.rename_preset(
                    &self.plugin_id,
                    preset_id,
                    name,
                )?)?)
            }
            "delete_preset" => {
                let preset_id = params
                    .get("preset_id")
                    .and_then(serde_json::Value::as_str)
                    .context("delete preset command is missing preset_id")?;
                Ok(serde_json::to_value(
                    store.delete_preset(&self.plugin_id, preset_id)?,
                )?)
            }
            "export_preset" => {
                let preset_id = params
                    .get("preset_id")
                    .and_then(serde_json::Value::as_str)
                    .context("export preset command is missing preset_id")?;
                let (file_name, file) = store.export_preset_file(
                    &self.plugin_id,
                    preset_id,
                    &self.runtime.0.manifest().name,
                )?;
                Ok(serde_json::json!({ "file_name": file_name, "file": file }))
            }
            "inspect_preset" => {
                let file: RfPresetFile = serde_json::from_value(
                    params
                        .get("file")
                        .cloned()
                        .context("inspect preset command is missing file")?,
                )?;
                Ok(serde_json::to_value(store.inspect_preset_file(
                    &self.plugin_id,
                    &self.plugin_version,
                    self.runtime.0.manifest().state_version,
                    &file,
                )?)?)
            }
            "import_preset" => {
                let file: RfPresetFile = serde_json::from_value(
                    params
                        .get("file")
                        .cloned()
                        .context("import preset command is missing file")?,
                )?;
                let policy: PresetImportConflictPolicy = serde_json::from_value(
                    params
                        .get("conflict_policy")
                        .cloned()
                        .unwrap_or_else(|| serde_json::json!("reject")),
                )?;
                Ok(serde_json::to_value(store.import_preset_file(
                    &self.plugin_id,
                    &self.plugin_version,
                    self.runtime.0.manifest().state_version,
                    &file,
                    policy,
                )?)?)
            }
            _ => bail!("unknown plugin state command {method:?}"),
        }
    }

    fn isolated_state_context(&self) -> AndroidIsolatedStateContext {
        AndroidIsolatedStateContext {
            runtime: Some(self.runtime),
            resource_overrides: self.resource_overrides.clone(),
            data_root: self.data_root.clone(),
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
        self.mirror_control(|instance| instance.reset())
            .context("resetting the program audition")?;
        self.mirror_control(|instance| instance.load_preset(&previous))
            .with_context(|| format!("restoring preset {previous:?}"))?;
        self.process_faulted = false;
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
        self.mirror_control(|instance| instance.install_program(&prepared))
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
                "master_level": MASTER_LEVEL_VALUE.load(Ordering::Relaxed),
                "master_pan": MASTER_PAN_VALUE.load(Ordering::Relaxed),
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
        let reset_rack = RACK_RESET_PENDING.swap(false, Ordering::AcqRel);
        if let Some(rack) = self.rack.as_mut() {
            // LIVE: the Rack plays in place of PLAY's voice and chain. Every
            // Slot hears the keyboard through its own stages, and links to
            // a Slot's parameters move that Slot.
            if reset_rack {
                rack.engine.reset();
            }
            if let Ok(mut queue) = midi_queue().try_lock() {
                for ingress in queue.drain(..) {
                    rack.engine.route(
                        IngressMidiEvent {
                            source: ingress.source,
                            packet: MidiPacket {
                                frame: ingress.event.frame,
                                length: ingress.event.length,
                                data: ingress.event.data,
                                wide: None,
                            },
                        },
                        None,
                    );
                }
            }
            let dropped = rack.engine.take_dropped_events();
            if dropped > 0 {
                MIDI_DROPPED_EVENTS.fetch_add(dropped, Ordering::Relaxed);
            }
            let deadline_ns = u64::from(frames) * 1_000_000_000 / SAMPLE_RATE as u64;
            // Android opens no audio input, so a Slot cabled to the input
            // hears silence.
            return rack.engine.render(
                &mut self.render_pool,
                &self.render_telemetry,
                frames,
                deadline_ns,
                None,
                output,
            );
        }
        if let Ok(mut queue) = midi_queue().try_lock() {
            for ingress in queue.drain(..) {
                let packet = MidiPacket {
                    frame: ingress.event.frame,
                    length: ingress.event.length,
                    data: ingress.event.data,
                    wide: None,
                };
                let ingress_event = IngressMidiEvent {
                    source: ingress.source,
                    packet,
                };
                let mut consume = false;
                for link in &mut self.parameter_links {
                    if link.link.instance_id != ANDROID_INSTANCE_ID {
                        continue;
                    }
                    // Where the parameter stands: a knob picks it up there,
                    // and lets go when a pad or the screen moves it.
                    let instance = &mut self.instance.0;
                    if let Some(output) =
                        link.apply(ingress_event, |index| instance.get_parameter(index).ok())
                    {
                        if self.parameter_events.len() < MAX_PENDING_MIDI_EVENTS {
                            self.parameter_events.push(output.event);
                            self.live_parameter_writer_handle.try_record(
                                0,
                                output.event.parameter_index,
                                output.event.value,
                            );
                        }
                        consume |= output.pass_through == ParameterLinkPassThrough::Consume;
                    }
                }
                if !consume {
                    self.midi.push(ingress.event);
                }
            }
        }
        let deadline_ns = u64::from(frames) * 1_000_000_000 / SAMPLE_RATE as u64;
        let was_faulted = self.process_faulted;
        self.wide.clear();
        self.wide
            .extend(self.midi.iter().map(Midi2Event::from_midi1));
        let mut slot = AndroidSlot {
            instance: &mut self.instance.0,
            parallel: self.parallel.as_mut().map(|parallel| &mut parallel.0),
            output: &mut *output,
            midi: &self.midi,
            wide: &self.wide,
            parameter_events: &self.parameter_events,
            faulted: &mut self.process_faulted,
        };
        let render_started = std::time::Instant::now();
        let scheduled =
            self.render_pool
                .process(std::slice::from_mut(&mut slot), frames, 2, deadline_ns);
        if !scheduled {
            process_slots_sequential(
                std::slice::from_mut(&mut slot),
                frames,
                2,
                &self.render_telemetry,
            );
            self.render_telemetry.record_block(
                render_started.elapsed().as_nanos() as u64,
                deadline_ns,
                None,
            );
        }
        if self.process_faulted && !was_faulted {
            eprintln!(
                "PLUGIN_PROCESS_QUARANTINED context=android:{} action=silence",
                self.plugin_id
            );
        }
        self.render_chain(frames, output);
        self.midi.clear();
        self.parameter_events.clear();
        Ok(())
    }

    fn replace_parameter_links(&mut self, links: Vec<ParameterLink>) -> Result<()> {
        let sources = midi_sources()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI source registry lock poisoned"))?;
        let mut compiled = Vec::with_capacity(links.len());
        for link in &links {
            link.validate()?;
            if link.instance_id != ANDROID_INSTANCE_ID {
                // Rack-slot links remain persisted and pending until Android's
                // corresponding isolated performance voice is active.
                continue;
            }
            let source_key = sources
                .resolve_optional(&link.source.source_id)
                .unwrap_or(VIRTUAL_MIDI_SOURCE_KEY);
            let candidate =
                CompiledParameterLink::new(link.clone(), source_key, self.runtime.0.parameters())?;
            if sources.resolve_optional(&link.source.source_id).is_some() {
                compiled.push(candidate);
            }
        }
        let controllers = midi_controllers()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI controller lock poisoned"))?;
        let maps = controller_maps()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller map lock poisoned"))?;
        for (source_id, controller) in controllers.iter() {
            let Some(controller_id) = controller.controller_id.as_deref() else {
                continue;
            };
            let Some(source_key) = sources.resolve_optional(source_id) else {
                continue;
            };
            let display_name = sources
                .descriptor(source_key)
                .map(|descriptor| descriptor.name.as_str())
                .unwrap_or(controller_id);
            // The player's map for this controller and plugin sits between
            // the session's own links and the package's defaults.
            let mut explicit_links = links.clone();
            if let Some(map) = maps.maps.get(controller_id) {
                let mapped = compile_controller_map_links(ControllerMapLinkContext {
                    map,
                    plugin_id: &self.plugin_id,
                    runtime_source_id: source_id,
                    source_name: display_name,
                    source_key,
                    instance_id: ANDROID_INSTANCE_ID,
                    schema: self.runtime.0.parameters(),
                    explicit_links: &links,
                });
                for (mapping, reason) in &mapped.pending {
                    eprintln!(
                        "CONTROLLER_MAPPING_PENDING controller={controller_id} plugin={} mapping={mapping} reason={reason:?}",
                        self.plugin_id
                    );
                }
                explicit_links.extend(mapped.links.iter().map(|link| link.link.clone()));
                compiled.extend(mapped.links);
            }
            let Some(profile) = &controller.profile else {
                continue;
            };
            compiled.extend(compile_semantic_parameter_links(
                SemanticParameterLinkContext {
                    controller_id,
                    controller_name: display_name,
                    profile,
                    runtime_source_id: source_id,
                    source_key,
                    instance_id: ANDROID_INSTANCE_ID,
                    schema: self.runtime.0.parameters(),
                    explicit_links: &explicit_links,
                },
            )?);
        }
        // Links to a Rack's Slots move them while the Rack is loaded; the
        // rest wait, as they always did.
        if let Some(rack) = self.rack.as_mut() {
            let rack_links =
                compile_rack_parameter_links(&rack.engine, &links, &sources, &controllers, &maps);
            rack.engine.set_parameter_links(rack_links);
        }
        // Every link takes a parameter over the way the player chose.
        for link in &mut compiled {
            link.set_takeover(maps.takeover);
        }
        self.persisted_parameter_links = links;
        self.parameter_links = compiled;
        Ok(())
    }

    /// What a touch did, named, when it names PLAY's instrument, one of its
    /// effects or a Slot of the Rack on stage: LITTLE's header and the web's
    /// window are both made from it.
    fn parameter_touch_report(
        &self,
        touch: &rackforge_core::parameter_touch::ParameterTouch,
    ) -> Option<ParameterTouchReport> {
        use rackforge_core::parameter_touch::{TouchPickup, touch_names};
        let (instance_id, runtime) = if touch_names(touch, ANDROID_INSTANCE_ID) {
            Some((ANDROID_INSTANCE_ID.to_owned(), self.runtime.0))
        } else if let Some(effect) = self
            .chain
            .iter()
            .find(|effect| touch_names(touch, &effect.instance_id))
        {
            Some((effect.instance_id.clone(), effect.runtime.0))
        } else {
            self.rack
                .as_ref()
                .and_then(|rack| rack.engine.touched_slot(touch))
                .map(|(slot_id, plugin)| (slot_id.to_owned(), plugin))
        }?;
        let schema = runtime.parameters();
        let parameter = schema
            .parameters
            .iter()
            .find(|parameter| parameter.index == touch.parameter_index)?
            .clone();
        Some(ParameterTouchReport {
            instance_id,
            parameter,
            value: touch.value,
            display_decimals: schema.display_decimals,
            pickup: match touch.pickup {
                TouchPickup::Engaged => ParameterTouchPickup::Engaged,
                TouchPickup::MoveUp => ParameterTouchPickup::MoveUp,
                TouchPickup::MoveDown => ParameterTouchPickup::MoveDown,
            },
            control: (touch.pickup != TouchPickup::Engaged).then_some(touch.control),
        })
    }

    /// LITTLE's header for a touch: the parameter's own name and value.
    fn parameter_touch_header(
        &self,
        touch: &rackforge_core::parameter_touch::ParameterTouch,
    ) -> Option<String> {
        let report = self.parameter_touch_report(touch)?;
        let arrow = match report.pickup {
            ParameterTouchPickup::Engaged => None,
            ParameterTouchPickup::MoveUp => Some(rackforge_surface_runtime::PickupArrow::Up),
            ParameterTouchPickup::MoveDown => Some(rackforge_surface_runtime::PickupArrow::Down),
        };
        Some(rackforge_surface_runtime::parameter_touch_header(
            &report.parameter,
            report.value,
            report.display_decimals,
            arrow,
            report.control,
        ))
    }

    /// Puts a built Rack on stage, with its Slots' links compiled; the Rack
    /// it replaces is handed back, to be let go outside the engine's lock.
    fn install_rack(&mut self, mut rack: AndroidRack) -> Result<Option<AndroidRack>> {
        let sources = midi_sources()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI source registry lock poisoned"))?;
        let controllers = midi_controllers()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI controller lock poisoned"))?;
        let maps = controller_maps()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller map lock poisoned"))?;
        let links = compile_rack_parameter_links(
            &rack.engine,
            &self.persisted_parameter_links,
            &sources,
            &controllers,
            &maps,
        );
        rack.engine.set_parameter_links(links);
        Ok(self.rack.replace(rack))
    }

    fn recompile_parameter_links(&mut self) -> Result<()> {
        self.replace_parameter_links(self.persisted_parameter_links.clone())
    }
}

/// The session's links that name one of this Rack's Slots, compiled against
/// that Slot's plugin, then each connected controller's map for the plugin
/// every Slot runs, as the Pi and the desktop play a Rack. A link from a
/// controller that is not connected waits.
fn compile_rack_parameter_links(
    rack: &RackEngine<'static>,
    links: &[ParameterLink],
    sources: &MidiSourceRegistry,
    controllers: &BTreeMap<MidiSourceId, AndroidMidiController>,
    maps: &AndroidControllerMaps,
) -> Vec<CompiledParameterLink> {
    let mut compiled: Vec<CompiledParameterLink> = links
        .iter()
        .filter_map(|link| {
            let plugin = rack.link_target_plugin(&link.instance_id)?;
            let source_key = sources.resolve_optional(&link.source.source_id)?;
            CompiledParameterLink::new(link.clone(), source_key, plugin.parameters())
                .map_err(|error| {
                    eprintln!(
                        "PARAMETER_LINK_PENDING link={} instance={} reason={error:#}",
                        link.id, link.instance_id
                    );
                })
                .ok()
        })
        .collect();
    for (source_id, controller) in controllers {
        let Some(map) = controller
            .controller_id
            .as_deref()
            .and_then(|controller_id| maps.maps.get(controller_id))
        else {
            continue;
        };
        let Some(source_key) = sources.resolve_optional(source_id) else {
            continue;
        };
        let source_name = sources
            .descriptor(source_key)
            .map(|descriptor| descriptor.name.as_str())
            .unwrap_or(source_id.as_str());
        for (voice_id, plugin_id) in rack.slots() {
            let Some(plugin) = rack.link_target_plugin(voice_id) else {
                continue;
            };
            // A map's link names the Slot, as a link made in the Rack
            // editor does, so the session's own link to it wins.
            let slot_id = voice_id.rsplit_once('/').map_or(voice_id, |(_, slot)| slot);
            compiled.extend(
                compile_controller_map_links(ControllerMapLinkContext {
                    map,
                    plugin_id,
                    runtime_source_id: source_id,
                    source_name,
                    source_key,
                    instance_id: slot_id,
                    schema: plugin.parameters(),
                    explicit_links: links,
                })
                .links,
            );
        }
    }
    for link in &mut compiled {
        link.set_takeover(maps.takeover);
    }
    compiled
}

/// Builds the Rack a LIVE location names: its graph compiled, each Slot's
/// plugin loaded from its installed package and its sound from the store,
/// every Slot activated. Nothing is locked while the plugins load.
fn build_android_rack(
    data_root: &Path,
    location: &rackforge_performance_api::LiveLocation,
    package_roots: &BTreeMap<String, String>,
) -> Result<(AndroidRack, serde_json::Value)> {
    let (library, rack) = {
        let guard = performance()
            .lock()
            .map_err(|_| anyhow::anyhow!("performance library lock poisoned"))?;
        let state = guard.as_ref().context("the LIVE library is not loaded")?;
        let library = state.repository.library().clone();
        let rack = library
            .resolve_playable(location)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        (library, rack)
    };
    build_android_rack_from(data_root, &library, &rack, package_roots)
}

/// A Rack the editor has not saved, built to be heard: the library as it
/// stands, with the draft in place of the Rack it edits.
fn build_android_preview_rack(
    data_root: &Path,
    draft: rackforge_performance_api::RackDefinition,
    package_roots: &BTreeMap<String, String>,
) -> Result<(AndroidRack, serde_json::Value)> {
    let mut library = {
        let guard = performance()
            .lock()
            .map_err(|_| anyhow::anyhow!("performance library lock poisoned"))?;
        guard
            .as_ref()
            .context("the LIVE library is not loaded")?
            .repository
            .library()
            .clone()
    };
    match library.racks.iter_mut().find(|rack| rack.id == draft.id) {
        Some(saved) => *saved = draft.clone(),
        None => library.racks.push(draft.clone()),
    }
    build_android_rack_from(data_root, &library, &draft, package_roots)
}

fn build_android_rack_from(
    data_root: &Path,
    library: &rackforge_performance_api::PerformanceLibrary,
    rack: &rackforge_performance_api::RackDefinition,
    package_roots: &BTreeMap<String, String>,
) -> Result<(AndroidRack, serde_json::Value)> {
    let store = PluginStateStore::new(Some(data_root))?;
    let specs = rack_voice::rack_runtime_specs(library, rack, &store)?;
    let mut plugins = BTreeMap::new();
    for spec in &specs {
        if plugins.contains_key(&spec.plugin_id) {
            continue;
        }
        let package_root = package_roots.get(&spec.plugin_id).with_context(|| {
            format!(
                "{} needs {}, which is not installed on this device",
                rack.name, spec.plugin_id
            )
        })?;
        let package = PluginPackage::open(Path::new(package_root))
            .with_context(|| format!("opening {}", spec.plugin_id))?;
        ensure_rack_slot_plugin(&package)?;
        let runtime = cached_isolated_plugin_runtime(&package, data_root)?;
        plugins.insert(spec.plugin_id.clone(), runtime.0);
    }
    let engine = RackEngine::build(&plugins, &specs, SAMPLE_RATE as u32, MAX_FRAMES, 2)
        .with_context(|| format!("building {}", rack.name))?;
    let summary = serde_json::json!({
        "rack_id": rack.id,
        "rack_name": rack.name,
        "slots": engine
            .slots()
            .map(|(slot_id, plugin_id)| serde_json::json!({
                "slot_id": slot_id,
                "plugin_id": plugin_id,
            }))
            .collect::<Vec<_>>(),
    });
    Ok((
        AndroidRack {
            rack_id: rack.id.as_str().to_owned(),
            engine,
        },
        summary,
    ))
}

/// Takes the Rack off the stage: PLAY's voice plays again. The Rack is let
/// go outside the engine's lock.
fn drop_android_rack() -> Result<()> {
    let retired = engine()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
        .as_mut()
        .and_then(|engine| engine.rack.take());
    if let Some(rack) = retired {
        println!("LIVE_RACK_RELEASED rack={}", rack.rack_id);
    }
    Ok(())
}

fn isolated_plugin_state_command(
    context: &AndroidIsolatedStateContext,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value> {
    if method == "materialize" {
        return materialize_isolated_plugin_state(context, params);
    }
    if method == "catalog" {
        return isolated_plugin_catalog(context, params);
    }
    let state: PluginStateReference = serde_json::from_value(
        params
            .get("state")
            .cloned()
            .context("isolated plugin parameter command is missing state")?,
    )?;
    state
        .validate()
        .context("validating isolated plugin state")?;
    let (runtime, resource_overrides) = isolated_plugin_runtime(context, params, &state)?;
    validate_state_reference(runtime.0, &state)?;
    let mut store = PluginStateStore::new(Some(&context.data_root))?;
    let bytes = store.read(&state)?;
    let mut editor = IsolatedPluginStateEditor::open(runtime.0, &resource_overrides, &bytes)?;

    match method {
        "plugin_state_parameters" => {
            let (schema, values) = editor.parameters()?;
            Ok(serde_json::to_value(
                ControlResponse::PluginStateParameters {
                    state: Box::new(state),
                    schema: Box::new(schema),
                    values,
                },
            )?)
        }
        "set_plugin_state_parameter" => {
            let parameter_index = params
                .get("parameter_index")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .context("set plugin state parameter command has an invalid parameter_index")?;
            let value = params
                .get("value")
                .and_then(serde_json::Value::as_f64)
                .filter(|value| value.is_finite())
                .context("set plugin state parameter command requires a finite value")?;
            let canonical = editor.set_parameter(parameter_index, value)?;
            let bytes = editor.save_state()?;
            let manifest = runtime.0.manifest();
            let next_state = store.put(
                &manifest.id,
                &manifest.version,
                manifest.state_version,
                state.selected_sound_id.clone(),
                &bytes,
            )?;
            Ok(serde_json::to_value(
                ControlResponse::PluginStateParameterSet {
                    state: Box::new(next_state),
                    parameter_index,
                    value: canonical,
                },
            )?)
        }
        _ => bail!("unknown isolated plugin state command {method:?}"),
    }
}

/// The programs of a plugin a Rack Slot holds, for the Rack editor: Android
/// runs one plugin at a time, so the session carries the programs of that one
/// alone, and a Slot holding any other would otherwise show none.
fn isolated_plugin_catalog(
    context: &AndroidIsolatedStateContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value> {
    let plugin_id = params
        .get("plugin_id")
        .and_then(serde_json::Value::as_str)
        .context("plugin catalog command is missing plugin_id")?;
    let (runtime, resource_overrides) = rack_slot_runtime(context, params, plugin_id)?;
    let catalog = runtime
        .0
        .create_instance_with_resource_overrides(&resource_overrides)?
        .preset_catalog()?;
    Ok(serde_json::json!({
        "plugin_id": plugin_id,
        "banks": catalog.banks.iter().map(|bank| serde_json::json!({
            "id": bank.id,
            "name": bank.name,
            "order": bank.order,
        })).collect::<Vec<_>>(),
        "sounds": catalog.presets.iter().map(|preset| serde_json::json!({
            "id": preset.id,
            "name": preset.name,
            "bank": preset.bank,
            "detail": preset.description.clone().or_else(|| preset.category.clone()),
            "editable": preset.editable,
        })).collect::<Vec<_>>(),
    }))
}

/// The runtime a Rack Slot's plugin runs in: the active plugin's own, or the
/// installed package's, loaded once and kept.
fn rack_slot_runtime(
    context: &AndroidIsolatedStateContext,
    params: &serde_json::Value,
    plugin_id: &str,
) -> Result<(SendableLoadedPlugin, BTreeMap<String, PathBuf>)> {
    if let Some(runtime) = context.active(plugin_id, None) {
        return Ok((runtime, context.resource_overrides.clone()));
    }
    let package_root = PathBuf::from(
        params
            .get("package_root")
            .and_then(serde_json::Value::as_str)
            .context("Rack Slot plugin command is missing package_root")?,
    );
    let package = PluginPackage::open(&package_root)
        .with_context(|| format!("opening Rack Slot plugin {}", package_root.display()))?;
    if package.manifest().id != plugin_id {
        bail!("Rack Slot plugin package identity does not match the requested plugin");
    }
    ensure_rack_slot_plugin(&package)?;
    Ok((
        cached_isolated_plugin_runtime(&package, &context.data_root)?,
        BTreeMap::new(),
    ))
}

/// What a Rack Slot may hold on Android: an instrument or an effect -- the
/// Rack editor offers both -- with a portable runtime to run it in.
fn ensure_rack_slot_plugin(package: &PluginPackage) -> Result<()> {
    let manifest = package.manifest();
    if manifest.portable_component().is_none() {
        bail!("{} has no portable wasm-v1 runtime", manifest.name);
    }
    if !matches!(manifest.kind, PluginKind::Instrument | PluginKind::Effect) {
        bail!("{} is neither an instrument nor an effect", manifest.name);
    }
    Ok(())
}

fn materialize_isolated_plugin_state(
    context: &AndroidIsolatedStateContext,
    params: &serde_json::Value,
) -> Result<serde_json::Value> {
    let plugin_id = params
        .get("plugin_id")
        .and_then(serde_json::Value::as_str)
        .context("materialize plugin state command is missing plugin_id")?;
    let (runtime, resource_overrides) = rack_slot_runtime(context, params, plugin_id)?;

    let requested_sound_id = params
        .get("sound_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let mut isolated = runtime
        .0
        .create_instance_with_resource_overrides(&resource_overrides)?;
    let catalog = isolated.preset_catalog()?;
    let sound_id = requested_sound_id.or_else(|| {
        catalog
            .presets
            .first()
            .or_else(|| runtime.0.presets().presets.first())
            .map(|preset| preset.id.clone())
    });
    if let Some(sound_id) = sound_id.as_deref() {
        isolated
            .load_preset(sound_id)
            .with_context(|| format!("loading Rack Slot sound {sound_id:?}"))?;
    }
    let bytes = isolated.save_state()?;
    let manifest = runtime.0.manifest();
    let mut store = PluginStateStore::new(Some(&context.data_root))?;
    let state = store.put(
        &manifest.id,
        &manifest.version,
        manifest.state_version,
        sound_id,
        &bytes,
    )?;
    Ok(serde_json::to_value(state)?)
}

fn isolated_plugin_runtime(
    context: &AndroidIsolatedStateContext,
    params: &serde_json::Value,
    state: &PluginStateReference,
) -> Result<(SendableLoadedPlugin, BTreeMap<String, PathBuf>)> {
    if let Some(runtime) = context.active(&state.plugin_id, Some(&state.plugin_version)) {
        return Ok((runtime, context.resource_overrides.clone()));
    }

    let store_root = PathBuf::from(
        params
            .get("plugin_store_root")
            .and_then(serde_json::Value::as_str)
            .context("isolated Rack Slot plugin is not active and plugin_store_root is missing")?,
    );
    let package_root = store_root
        .join("packages")
        .join(&state.plugin_id)
        .join(&state.plugin_version);
    let package = PluginPackage::open(&package_root)
        .with_context(|| format!("opening Rack Slot plugin {}", package_root.display()))?;
    let manifest = package.manifest();
    if manifest.id != state.plugin_id || manifest.version != state.plugin_version {
        bail!("Rack Slot plugin package identity does not match its state reference");
    }
    ensure_rack_slot_plugin(&package)?;

    Ok((
        cached_isolated_plugin_runtime(&package, &context.data_root)?,
        BTreeMap::new(),
    ))
}

fn cached_isolated_plugin_runtime(
    package: &PluginPackage,
    data_root: &Path,
) -> Result<SendableLoadedPlugin> {
    let manifest = package.manifest();
    let cache_key = format!("{}@{}", manifest.id, manifest.version);
    let mut runtimes = ISOLATED_PLUGIN_RUNTIMES
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| anyhow::anyhow!("isolated plugin runtime cache lock poisoned"))?;
    let runtime = if let Some(runtime) = runtimes.get(&cache_key).copied() {
        runtime
    } else {
        // SAFETY: Android accepts only validated portable wasm-v1 packages here.
        // The loaded runtime is immutable and retained for the process lifetime so
        // repeated slider input does not recompile the same Wasm component.
        let loaded =
            unsafe { LoadedPlugin::load(package, None, &BTreeMap::new(), Some(data_root)) }
                .context("loading isolated Rack Slot plugin runtime")?;
        let runtime = SendableLoadedPlugin(Box::leak(Box::new(loaded)));
        runtimes.insert(cache_key, runtime);
        runtime
    };
    Ok(runtime)
}

fn engine() -> &'static Mutex<Option<AndroidEngine>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

fn audio() -> &'static Mutex<Option<NativeAudioOutput>> {
    AUDIO.get_or_init(|| Mutex::new(None))
}

fn midi_queue() -> &'static Mutex<VecDeque<AndroidMidiIngress>> {
    MIDI_QUEUE.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_PENDING_MIDI_EVENTS)))
}

fn midi_sources() -> &'static Mutex<MidiSourceRegistry> {
    MIDI_SOURCES.get_or_init(|| Mutex::new(MidiSourceRegistry::default()))
}

fn midi_controllers() -> &'static Mutex<BTreeMap<MidiSourceId, AndroidMidiController>> {
    MIDI_CONTROLLERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn controller_maps() -> &'static Mutex<AndroidControllerMaps> {
    CONTROLLER_MAPS.get_or_init(|| Mutex::new(AndroidControllerMaps::default()))
}

fn midi_activity() -> &'static Mutex<MidiActivityLog> {
    MIDI_ACTIVITY.get_or_init(|| Mutex::new(MidiActivityLog::default()))
}

fn controller_store_root() -> Option<PathBuf> {
    CONTROLLER_STORE_ROOT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()?
        .clone()
}

/// Loads the player's maps from `data_root`, once: Java names the root with
/// every controller request, and the first one loads them.
fn ensure_controller_maps(data_root: &Path) -> Result<bool> {
    let mut maps = controller_maps()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller map lock poisoned"))?;
    if maps.data_root.as_deref() == Some(data_root) {
        return Ok(false);
    }
    let store = ControllerMapStore::new(Some(data_root));
    offer_android_factory_maps(&store, &maps);
    maps.maps = store
        .load_all()
        .map_err(|error| anyhow::anyhow!("loading controller maps: {error:#}"))?;
    maps.takeover = store.takeover();
    maps.store = Some(store);
    maps.data_root = Some(data_root.to_path_buf());
    Ok(true)
}

/// Offers each keyboard the map made from the installed plugins' control
/// layouts. An offer that fails costs the player nothing they had: their
/// maps still load.
fn offer_android_factory_maps(store: &ControllerMapStore, maps: &AndroidControllerMaps) {
    use rackforge_core::controller_layouts::{control_layouts, factory_maps, slotted_controllers};
    let packages = maps
        .plugin_store_root
        .as_deref()
        .map(installed_plugin_packages)
        .unwrap_or_default();
    let layouts = control_layouts(
        packages
            .iter()
            .map(|package| (package.manifest().id.as_str(), package.root())),
    );
    let controllers = slotted_controllers(maps.controller_store_root.as_deref());
    match store.seed_factory_maps(&factory_maps(&controllers, &layouts)) {
        Ok(seeded) if !seeded.is_empty() => {
            println!("FACTORY_CONTROLLER_MAPS_SEEDED controllers={seeded:?}");
        }
        Ok(_) => {}
        Err(error) => eprintln!("FACTORY_CONTROLLER_MAPS_FAILED error={error:#}"),
    }
}

/// After a plugin is installed or removed: what each keyboard is offered
/// changes, the stored maps take it in, and the links follow them.
fn reoffer_android_factory_maps() -> Result<()> {
    {
        let mut maps = controller_maps()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller map lock poisoned"))?;
        let Some(store) = maps.store.clone() else {
            return Ok(());
        };
        offer_android_factory_maps(&store, &maps);
        maps.maps = store
            .load_all()
            .map_err(|error| anyhow::anyhow!("loading controller maps: {error:#}"))?;
    }
    recompile_engine_parameter_links()
}

/// The newest enabled package of every installed plugin.
fn installed_plugin_packages(store_root: &Path) -> Vec<PluginPackage> {
    let Ok(entries) = std::fs::read_dir(store_root.join("packages")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let plugin_id = entry.file_name().into_string().ok()?;
            installed_package(store_root, &plugin_id).ok()
        })
        .collect()
}

/// Stores how knobs and faders take a parameter over and applies it at once.
fn set_android_controller_takeover(takeover: ControlTakeover) -> Result<()> {
    {
        let mut maps = controller_maps()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller map lock poisoned"))?;
        maps.store
            .as_ref()
            .context("controller maps are not loaded")?
            .set_takeover(takeover)
            .map_err(|error| {
                anyhow::anyhow!("Could not store the controller takeover: {error:#}")
            })?;
        maps.takeover = takeover;
    }
    recompile_engine_parameter_links()
}

fn recompile_engine_parameter_links() -> Result<()> {
    if let Some(engine) = engine()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
        .as_mut()
    {
        engine.recompile_parameter_links()?;
    }
    Ok(())
}

/// Resolves every connected source's package again: after the player made or
/// allowed one.
fn reresolve_midi_controllers() -> Result<()> {
    {
        let mut controllers = midi_controllers()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI controller lock poisoned"))?;
        for controller in controllers.values_mut() {
            controller.resolve();
        }
    }
    recompile_engine_parameter_links()
}

fn controller_menu() -> &'static Mutex<AndroidControllerMenu> {
    CONTROLLER_MENU.get_or_init(|| Mutex::new(AndroidControllerMenu::default()))
}

fn controller_parameter_mapper() -> &'static Mutex<RackForgeParameterMapper> {
    static MAPPER: OnceLock<Mutex<RackForgeParameterMapper>> = OnceLock::new();
    MAPPER.get_or_init(|| Mutex::new(RackForgeParameterMapper::default()))
}

fn apply_rackforge_parameter(parameter: RackForgeParameterValue) {
    match parameter {
        RackForgeParameterValue::MasterLevel(level) => {
            MASTER_LEVEL_VALUE.store(u32::from(level.get()), Ordering::Relaxed);
            MASTER_LEVEL_TARGET_BITS.store(level.amplitude().to_bits(), Ordering::Relaxed);
        }
        RackForgeParameterValue::MasterPan(pan) => {
            MASTER_PAN_VALUE.store(i32::from(pan.get()), Ordering::Relaxed);
            let (left, right) = pan.balance();
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

fn enqueue_midi(source: MidiSourceKey, bytes: &[u8]) {
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
        queue.push_back(AndroidMidiIngress {
            source,
            event: MidiEventV1 {
                frame: 0,
                length: bytes.len() as u8,
                data,
            },
        });
    }
}

fn apply_declarative_rackforge_parameter(source: MidiSourceKey, message: &[u8]) {
    let profile = midi_sources()
        .lock()
        .ok()
        .and_then(|sources| {
            sources
                .descriptor(source)
                .map(|descriptor| descriptor.id.clone())
        })
        .and_then(|source_id| {
            midi_controllers().lock().ok().and_then(|controllers| {
                controllers
                    .get(&source_id)
                    .and_then(|controller| controller.profile.clone())
            })
        });
    let Some(input) = profile
        .as_ref()
        .and_then(|profile| rackforge_parameter_input(profile, message))
    else {
        return;
    };
    let current_pan = MasterPan::new(MASTER_PAN_VALUE.load(Ordering::Relaxed) as i16)
        .unwrap_or(MasterPan::CENTER);
    if let Ok(mut mapper) = controller_parameter_mapper().lock()
        && let Some(parameter) = mapper.apply(input, current_pan)
    {
        apply_rackforge_parameter(parameter);
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
            queue.push_back(AndroidMidiIngress {
                source: VIRTUAL_MIDI_SOURCE_KEY,
                event: MidiEventV1 {
                    frame: 0,
                    length: packet.length,
                    data: packet.data,
                },
            });
        }
        MIDI_PANIC_COUNT.fetch_add(1, Ordering::Relaxed);
    }
    RACK_RESET_PENDING.store(true, Ordering::Release);
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
    AUDIO_STREAM_RECOVERY.mark_lost();
}

fn audio_status_json() -> String {
    let stream_recovery = AUDIO_STREAM_RECOVERY.snapshot();
    let stream_health = match stream_recovery.health {
        AudioStreamHealth::Healthy => "healthy",
        AudioStreamHealth::Lost => "lost",
        AudioStreamHealth::Recovering => "recovering",
    };
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
            "stream_health": stream_health,
            "stream_losses": stream_recovery.losses,
            "stream_recoveries": stream_recovery.recoveries,
        })
        .to_string();
    };
    let render_queue = output.callback_context.queue.snapshot();
    let dropout_recovery = AUDIO_DROPOUT_RECOVERY.snapshot();
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
            "device_id": AAudioStream_getDeviceId(output.stream),
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
            "render_queue_frames": render_queue.queued_frames,
            "render_queue_underruns": render_queue.underrun_callbacks,
            "render_queue_underrun_frames": render_queue.underrun_frames,
            "render_queue_saturated_pushes": render_queue.saturated_pushes,
            "render_queue_invalid_pushes": render_queue.invalid_pushes,
            "dropout_concealed_callbacks": dropout_recovery.concealed_callbacks,
            "dropout_recovered_callbacks": dropout_recovery.recovered_callbacks,
            "stream_health": stream_health,
            "stream_losses": stream_recovery.losses,
            "stream_recoveries": stream_recovery.recoveries,
            "render_thread_priority_result": AUDIO_RENDER_THREAD_PRIORITY_RESULT.load(Ordering::Relaxed),
            "callback_count": callback_count,
            "average_callback_us": average_callback_micros,
            "maximum_callback_us": AUDIO_CALLBACK_MAX_NANOS.load(Ordering::Relaxed) as f64 / 1_000.0,
            "callback_overruns": AUDIO_CALLBACK_OVERRUNS.load(Ordering::Relaxed),
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
        AUDIO_DROPOUT_RECOVERY.conceal(&mut output[rendered_frames * 2..], DROPOUT_FADE_FRAMES);
    } else {
        AUDIO_DROPOUT_RECOVERY.recover(output, DROPOUT_FADE_FRAMES);
    }
    let output_gain = f32::from_bits(OUTPUT_GAIN_BITS.load(Ordering::Relaxed));
    let level_target = f32::from_bits(MASTER_LEVEL_TARGET_BITS.load(Ordering::Relaxed));
    let pan_left_target = f32::from_bits(MASTER_PAN_LEFT_TARGET_BITS.load(Ordering::Relaxed));
    let pan_right_target = f32::from_bits(MASTER_PAN_RIGHT_TARGET_BITS.load(Ordering::Relaxed));
    let mut level = f32::from_bits(MASTER_LEVEL_CURRENT_BITS.load(Ordering::Relaxed));
    let mut pan_left = f32::from_bits(MASTER_PAN_LEFT_CURRENT_BITS.load(Ordering::Relaxed));
    let mut pan_right = f32::from_bits(MASTER_PAN_RIGHT_CURRENT_BITS.load(Ordering::Relaxed));
    let mut nonfinite = 0_u64;
    let mut clipped = 0_u64;
    let mut left_peak = 0.0_f32;
    let mut right_peak = 0.0_f32;
    for (index, frame) in output.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        smooth_master_sample(&mut level, level_target);
        smooth_master_sample(&mut pan_left, pan_left_target);
        smooth_master_sample(&mut pan_right, pan_right_target);
        let gain = output_gain * level;
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
        if left.is_finite() {
            left_peak = left_peak.max(left.abs());
        }
        if right.is_finite() {
            right_peak = right_peak.max(right.abs());
        }
        for (sample, value) in frame.iter_mut().zip([left, right]) {
            if !value.is_finite() {
                *sample = 0.0;
                nonfinite += 1;
            } else {
                // Still counted the moment the mix asks for more than full
                // scale, which is the thing worth reporting; what changed is
                // that asking no longer squares the wave off. See
                // `output_ceiling`: a brick wall is a high-order
                // nonlinearity, and on a chord it invents energy below the
                // lowest note being played.
                if value.abs() > 1.0 {
                    clipped += 1;
                }
                *sample = output_ceiling(value);
            }
        }
    }
    AUDIO_NONFINITE_SAMPLES.fetch_add(nonfinite, Ordering::Relaxed);
    AUDIO_CLIPPED_SAMPLES.fetch_add(clipped, Ordering::Relaxed);
    OUTPUT_METER.observe_stereo(left_peak, right_peak);
    MASTER_LEVEL_CURRENT_BITS.store(level.to_bits(), Ordering::Relaxed);
    MASTER_PAN_LEFT_CURRENT_BITS.store(pan_left.to_bits(), Ordering::Relaxed);
    MASTER_PAN_RIGHT_CURRENT_BITS.store(pan_right.to_bits(), Ordering::Relaxed);
    AUDIO_DROPOUT_RECOVERY.remember_last_frame(output);
    let elapsed = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
    AUDIO_CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
    AUDIO_CALLBACK_FRAMES.fetch_add(num_frames as u64, Ordering::Relaxed);
    AUDIO_CALLBACK_TOTAL_NANOS.fetch_add(elapsed, Ordering::Relaxed);
    AUDIO_CALLBACK_MAX_NANOS.fetch_max(elapsed, Ordering::Relaxed);
    let callback_budget_nanos = (num_frames as u64)
        .saturating_mul(1_000_000_000)
        .checked_div(SAMPLE_RATE as u64)
        .unwrap_or(0);
    if callback_budget_nanos > 0 && elapsed > callback_budget_nanos {
        AUDIO_CALLBACK_OVERRUNS.fetch_add(1, Ordering::Relaxed);
    }
    AAUDIO_CALLBACK_RESULT_CONTINUE
}

fn java_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Result<String> {
    Ok(env.get_string(&value)?.into())
}

fn report(env: &mut JNIEnv<'_>, error: anyhow::Error) {
    let _ = env.throw_new("java/lang/IllegalStateException", format!("{error:#}"));
}

fn package_descriptor(package: &PluginPackage, active: bool) -> serde_json::Value {
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
    // An effect never takes the stage, but PLAY's chain can hold it, and the
    // two things it needs to be held are the two the stage asks for.
    let chainable = manifest.kind == PluginKind::Effect && portable && play_entry.is_some();
    let incompatibility = if compatible || chainable {
        None
    } else if !portable {
        Some("Android requires a portable wasm-v1 component")
    } else if play_entry.is_none() {
        Some("The plugin does not provide a PLAY Web surface")
    } else {
        Some("Android PLAY plays instruments and the effects after them")
    };
    let root = package.root();
    serde_json::json!({
        "plugin_id": manifest.id,
        "plugin_name": manifest.name,
        "plugin_short_name": manifest.little_short_name(),
        "version": manifest.version,
        "kind": manifest.kind,
        "portable": portable,
        "compatible": compatible,
        "chainable": chainable,
        "suggested_chain": manifest.suggested_chain,
        "incompatibility": incompatibility,
        "package_root": root.to_string_lossy(),
        "web_entry": play_entry,
        "config_web_entry": config_entry,
        "web_api_version": manifest.web_ui.as_ref().map_or(0, |web| web.api_version),
        "branding": manifest.branding,
        "resources": manifest.resources,
        "active": active,
    })
}

/// The newest installed version of `plugin_id`, when the store has it and
/// the player has not switched it off.
///
/// PLAY's chain names plugins, not packages: an effect is found here the way
/// the catalog finds one, by reading each version's own manifest.
fn installed_package(store_root: &Path, plugin_id: &str) -> Result<PluginPackage> {
    if !plugin_is_enabled(store_root, plugin_id).unwrap_or(true) {
        bail!("plugin {plugin_id} is switched off");
    }
    let versions = store_root.join("packages").join(plugin_id);
    let mut newest: Option<(semver::Version, PathBuf)> = None;
    for entry in std::fs::read_dir(&versions)
        .with_context(|| format!("reading {}", versions.display()))?
        .flatten()
    {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(package) = PluginPackage::open(entry.path()) else {
            continue;
        };
        let Ok(version) = semver::Version::parse(&package.manifest().version) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(newest, _)| version > *newest) {
            newest = Some((version, entry.path()));
        }
    }
    let (_, root) = newest.with_context(|| format!("plugin {plugin_id} is not installed"))?;
    PluginPackage::open(&root).with_context(|| format!("opening {}", root.display()))
}

fn installed_plugins_json(store_root: &Path) -> Result<String> {
    let _ = cleanup_uninstall_tombstones(store_root);
    let packages_root = store_root.join("packages");
    std::fs::create_dir_all(&packages_root)
        .with_context(|| format!("creating {}", packages_root.display()))?;
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
                    Ok(parsed) => {
                        let active =
                            plugin_is_enabled(store_root, &package.manifest().id).unwrap_or(true);
                        versions
                            .entry(package.manifest().id.clone())
                            .or_default()
                            .push((parsed, package_descriptor(&package, active)));
                    }
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

/// The performance library (Racks, Songs, Setlists) on Android: the same
/// repository format every platform persists, under the plugin data root.
/// Android has no live rack engine yet, so the definitions are what this
/// carries; activation maps to the single hosted instance.
struct AndroidPerformance {
    repository: PerformanceRepository,
    live: LivePerformanceState,
}

fn performance() -> &'static Mutex<Option<AndroidPerformance>> {
    PERFORMANCE.get_or_init(|| Mutex::new(None))
}

fn performance_snapshot(state: &AndroidPerformance) -> PerformanceSnapshot {
    PerformanceSnapshot {
        schema_version: PERFORMANCE_SNAPSHOT_SCHEMA_VERSION,
        revision: state.repository.revision(),
        library: state.repository.library().clone(),
        live: state.live.clone(),
    }
}

/// Loads (or keeps) the repository and hands the KeyLab menu the snapshot.
/// Idempotent: every caller that knows the data root may invoke it.
fn ensure_performance_menu(data_root: &Path) -> Result<()> {
    let snapshot = {
        let mut guard = performance()
            .lock()
            .map_err(|_| anyhow::anyhow!("performance library lock poisoned"))?;
        if guard.is_none() {
            let repository = PerformanceRepository::load_or_empty(Some(data_root))?;
            // Android loads a Rack only when asked, so none is active at start:
            // the library must never say a Rack sounds that does not.
            let mut live = repository.initial_live_state();
            live.deactivate();
            *guard = Some(AndroidPerformance { repository, live });
        }
        guard
            .as_ref()
            .map(performance_snapshot)
            .expect("the performance library was just initialized")
    };
    controller_menu()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))?
        .menu
        .sync_performance_snapshot(snapshot);
    Ok(())
}

/// Runs one engine call under the lock and releases it before any menu work,
/// keeping the menu -> engine lock order the only nesting that exists.
fn engine_call<T>(act: impl FnOnce(&mut AndroidEngine) -> Result<T>) -> Result<T> {
    let mut guard = engine()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
    act(guard
        .as_mut()
        .context("RackForge engine is not initialized")?)
}

fn set_android_little_parameter(
    instance_id: &str,
    parameter_index: u32,
    value: f64,
) -> Result<f64> {
    let mut guard = engine()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
    let engine = guard
        .as_mut()
        .context("RackForge engine is not initialized")?;
    // An effect of the chain takes the write in its own voice; the value is
    // the chain's, and is not persisted with the instrument's.
    if let Some(position) = engine
        .chain
        .iter()
        .position(|voice| voice.instance_id == instance_id)
    {
        let runtime = engine.chain[position].runtime;
        return set_plugin_parameter(
            runtime.0,
            &mut engine.chain[position].instance.0,
            parameter_index,
            value,
        );
    }
    if instance_id != engine.plugin_id {
        bail!("LITTLE targeted inactive plugin instance {instance_id}");
    }
    let requested = value;
    let value = set_plugin_parameter(
        engine.runtime.0,
        &mut engine.instance.0,
        parameter_index,
        requested,
    )?;
    if let Some(parallel) = engine.parallel.as_mut() {
        parallel
            .0
            .mirror(|instance| instance.set_parameter(parameter_index, requested))?;
    }
    engine
        .live_parameter_writer_handle
        .try_record(0, parameter_index, value);
    Ok(value)
}

fn sync_menu_parameter_state(menu: &mut SurfaceMenu) -> Result<()> {
    // Whose parameters LITTLE is showing: an effect of the chain while its
    // panel is open, and the instrument on stage otherwise.
    let target = menu.parameter_target_instance_id().map(str::to_owned);
    let (schema, values) = {
        let mut guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        let engine = guard
            .as_mut()
            .context("RackForge engine is not initialized")?;
        let position = target.and_then(|id| {
            engine
                .chain
                .iter()
                .position(|voice| voice.instance_id == id)
        });
        match position {
            Some(position) => {
                let runtime = engine.chain[position].runtime;
                plugin_parameters(runtime.0, &mut engine.chain[position].instance.0)?
            }
            None => plugin_parameters(engine.runtime.0, &mut engine.instance.0)?,
        }
    };
    menu.sync_plugin_parameters(
        schema,
        values.into_iter().map(|value| (value.index, value.value)),
    );
    Ok(())
}

/// After a program command the menu re-reads the engine: catalog, selection
/// and the draft -- the exact refresh the process-driver bridge performs.
fn sync_menu_program_state(menu: &mut SurfaceMenu) -> Result<()> {
    let (plugin_id, name, sounds, selected_sound_id, draft) = {
        let mut guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        let engine = guard
            .as_mut()
            .context("RackForge engine is not initialized")?;
        (
            engine.plugin_id.clone(),
            engine.plugin_name.clone(),
            controller_play_sounds(&engine.catalog),
            engine.selected_sound_id.clone(),
            engine.program_draft.clone(),
        )
    };
    let lease = draft.as_ref().map(|draft| draft.draft_id);
    if menu.sync_active_plugin(
        plugin_id.clone(),
        plugin_id,
        name,
        sounds,
        Some(&selected_sound_id),
    ) {
        menu.sync_program_edit(draft, lease);
    }
    sync_menu_parameter_state(menu)?;
    Ok(())
}

fn sync_controller_plugins(store_root: &Path) -> Result<()> {
    let catalog: serde_json::Value = serde_json::from_str(&installed_plugins_json(store_root)?)?;
    let active = engine()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
        .as_ref()
        .map(|engine| {
            let presets = PluginStateStore::new(Some(&engine.data_root))
                .and_then(|store| store.list_presets(&engine.plugin_id))
                .unwrap_or_default();
            (
                engine.package_root.to_string_lossy().into_owned(),
                engine.plugin_id.clone(),
                engine.plugin_name.clone(),
                engine.catalog.clone(),
                engine.selected_sound_id.clone(),
                engine.instance.0.supports_program_editing(),
                presets,
            )
        });
    let chain = engine()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
        .as_ref()
        .map(AndroidEngine::little_chain_items)
        .unwrap_or_default();
    // The effects PLAY's chain can take: installed, switched on, and of a
    // kind this host builds a voice for.
    let effect_plugins = catalog["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|plugin| {
            plugin["chainable"].as_bool().unwrap_or(false)
                && plugin["active"].as_bool().unwrap_or(false)
        })
        .filter_map(|descriptor| {
            let plugin_id = descriptor["plugin_id"].as_str()?;
            let name = descriptor["plugin_name"].as_str()?;
            let short_name = descriptor["plugin_short_name"].as_str().unwrap_or(name);
            Some(
                PlayPlugin::new(plugin_id, plugin_id, name)
                    .short_name(short_name)
                    .config_available(false),
            )
        })
        .collect::<Vec<_>>();
    let mut plugins = Vec::new();
    let mut metadata = BTreeMap::new();
    for descriptor in catalog["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|plugin| {
            plugin["compatible"].as_bool().unwrap_or(false)
                && plugin["active"].as_bool().unwrap_or(false)
        })
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
        let short_name = descriptor["plugin_short_name"].as_str().unwrap_or(name);
        let version = descriptor["version"].as_str().unwrap_or_default();
        let config_available = active
            .as_ref()
            .is_some_and(|active| active.1 == plugin_id && active.5);
        plugins.push(
            PlayPlugin::new(plugin_id, plugin_id, name)
                .short_name(short_name)
                .config_available(config_available),
        );
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
    controller.menu.set_play_effect_plugins(effect_plugins);
    controller.menu.set_play_chain(chain);
    if let Some((_root, plugin_id, name, catalog, selected_sound_id, _supports, presets)) = active {
        controller.menu.sync_active_plugin(
            plugin_id.clone(),
            plugin_id,
            name,
            controller_play_sounds(&catalog),
            Some(&selected_sound_id),
        );
        controller.menu.set_plugin_presets(
            presets
                .into_iter()
                .map(|preset| {
                    PlayPreset::new(
                        preset.id,
                        preset.name,
                        format!("v{}", preset.plugin_version),
                    )
                })
                .collect(),
            None,
        );
        sync_menu_parameter_state(&mut controller.menu)?;
    }
    Ok(())
}

fn sync_controller_active_plugin() -> Result<()> {
    let mut controller = controller_menu()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))?;
    sync_menu_program_state(&mut controller.menu)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabAcquirePlan(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = (|| -> Result<String> {
        let mut controller = controller_menu()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))?;
        let mut messages = keylab_protocol::acquire_messages().map_err(anyhow::Error::msg)?;
        // LITTLE opens on what is playing each time the KeyLab is taken.
        controller.menu.show_active_mode();
        let screen = controller.menu.render();
        controller
            .compositor
            .publish(screen, SurfaceUpdatePriority::Immediate);
        controller.compositor.invalidate_delivery();
        messages.extend(controller.render_plan(SurfaceUpdatePriority::Immediate)?);
        controller_plan_json(Ok(messages))
    })();
    result_string(&mut env, result)
}

/// The controller store: the SAME `.rfcontroller` package system every
/// platform uses. Android cannot run process drivers (no exec from
/// writable storage, and the MIDI transport is the Java API), so the
/// driver's role is played in-process by the shared protocol crate -- but
/// the package, the store layout, the settings schema and the JSON the UI
/// sees are identical to the desktop and the Pi.
fn controller_store_settings_path(store_root: &Path, controller_id: &str) -> PathBuf {
    store_root
        .join("state")
        .join(controller_id)
        .join("settings.toml")
}

fn read_controller_store_settings(
    store_root: &Path,
    controller_id: &str,
) -> BTreeMap<String, String> {
    fs::read_to_string(controller_store_settings_path(store_root, controller_id))
        .ok()
        .and_then(|text| toml::from_str::<BTreeMap<String, String>>(&text).ok())
        .unwrap_or_default()
}

/// What a setting MEANS on this hardware, applied in-process: the exact
/// mapping the process driver performs on the other platforms.
fn apply_controller_settings_in_process(values: &BTreeMap<String, String>) {
    if let Some(value) = values.get("key-light-color")
        && let Some(digits) = value.strip_prefix('#')
        && digits.len() == 6
        && let Ok(parsed) = u32::from_str_radix(digits, 16)
    {
        keylab_protocol::set_ambient_led_rgb([
            (((parsed >> 16) & 0xFF) as u8) >> 1,
            (((parsed >> 8) & 0xFF) as u8) >> 1,
            ((parsed & 0xFF) as u8) >> 1,
        ]);
    }
}

/// Installs the bundled KeyLab package into the store (idempotent) and
/// applies its stored settings. Called at boot before any MIDI session.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_ensureBundledControllers(
    mut env: JNIEnv,
    _class: JClass,
    store_root: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let store_root: String = env
            .get_string(&store_root)
            .context("reading controller store root")?
            .into();
        let store_root = PathBuf::from(store_root);
        *CONTROLLER_STORE_ROOT
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| anyhow::anyhow!("controller store root lock poisoned"))? =
            Some(store_root.clone());
        // The controllers RackForge describes from their makers'
        // documentation, installed as its own beside the KeyLab.
        rackforge_controller_catalog::install_bundled_and_report(&store_root);
        let store = rackforge_controller_package::PackageStore::new(&store_root);
        let manifest = rackforge_controller_package::stamp_bundled_manifest(
            keylab_essential_mk3::controller::PACKAGE_MANIFEST,
            &[],
        )
        .context("stamping the bundled controller manifest")?;
        let parsed: rackforge_controller_package::ControllerPackageManifest =
            toml::from_str(&manifest).context("parsing the bundled controller manifest")?;
        let already = store
            .list()
            .map(|installed| {
                installed.iter().any(|controller| {
                    controller.record.id == parsed.id && controller.record.version == parsed.version
                })
            })
            .unwrap_or(false);
        if !already {
            let staging = store_root.join("staging").join(&parsed.id);
            fs::create_dir_all(&staging)?;
            fs::write(
                staging.join(rackforge_controller_package::CONTROLLER_MANIFEST_FILE),
                &manifest,
            )?;
            store
                .install_directory(
                    &staging,
                    rackforge_controller_package::PackageTrust::Official,
                )
                .map_err(|error| anyhow::anyhow!("installing bundled controller: {error}"))?;
            let _ = fs::remove_dir_all(store_root.join("staging"));
        }
        let values = read_controller_store_settings(&store_root, &parsed.id);
        apply_controller_settings_in_process(&values);
        Ok(serde_json::json!({"status": "ok", "installed": !already}).to_string())
    })();
    result_string(&mut env, result)
}

/// Installs a controller package directory into the store: the same job
/// `--install-controller` performs on the desktop, reachable from adb via
/// the app's install inbox. Local packages carry official trust.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_controllerInstallDirectory(
    mut env: JNIEnv,
    _class: JClass,
    store_root: JString,
    package_dir: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let store_root: String = env
            .get_string(&store_root)
            .context("reading controller store root")?
            .into();
        let package_dir: String = env
            .get_string(&package_dir)
            .context("reading controller package path")?
            .into();
        let store = rackforge_controller_package::PackageStore::new(PathBuf::from(store_root));
        match store.install_directory(
            PathBuf::from(&package_dir),
            rackforge_controller_package::PackageTrust::Official,
        ) {
            Ok(installed) => Ok(serde_json::json!({
                "status": "ok",
                "id": installed.record.id,
                "version": installed.record.version,
                "already_installed": false,
            })
            .to_string()),
            Err(error) => {
                let text = error.to_string();
                if text.contains("already installed") {
                    Ok(serde_json::json!({
                        "status": "ok",
                        "already_installed": true,
                    })
                    .to_string())
                } else {
                    Err(anyhow::anyhow!("installing controller package: {text}"))
                }
            }
        }
    })();
    result_string(&mut env, result)
}

/// The installed controllers with their settings schema and current values:
/// the same JSON shape `GET /api/v1/controllers` serves on the desktop.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_controllerCatalog(
    mut env: JNIEnv,
    _class: JClass,
    store_root: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let store_root: String = env
            .get_string(&store_root)
            .context("reading controller store root")?
            .into();
        let store_root = PathBuf::from(store_root);
        let installed = rackforge_controller_package::PackageStore::new(&store_root)
            .list()
            .map_err(|error| anyhow::anyhow!("listing controller store: {error}"))?;
        let controllers: Vec<serde_json::Value> = installed
            .iter()
            .map(|controller| {
                let manifest = controller.package.manifest();
                let stored = read_controller_store_settings(&store_root, &controller.record.id);
                let settings: Vec<serde_json::Value> = manifest
                    .settings
                    .iter()
                    .map(|setting| {
                        serde_json::json!({
                            "id": setting.id,
                            "name": setting.name,
                            "kind": format!("{:?}", setting.kind).to_ascii_lowercase(),
                            "default": setting.default,
                            "page": setting.page,
                            "value": stored
                                .get(&setting.id)
                                .cloned()
                                .unwrap_or_else(|| setting.default.clone()),
                        })
                    })
                    .collect();
                // The controls themselves, for the Controllers editor.
                let editor = manifest.editor_inputs();
                serde_json::json!({
                    "id": controller.record.id,
                    "name": manifest.name,
                    "vendor": manifest.vendor,
                    "schema_version": manifest.schema_version,
                    "version": controller.record.version,
                    "enabled": controller.record.enabled,
                    "trust": format!("{:?}", controller.record.trust).to_ascii_lowercase(),
                    "runtime": if manifest.is_declarative() {
                        "DeclarativeV1"
                    } else {
                        "InProcess"
                    },
                    "devices": manifest.devices.len(),
                    "settings": settings,
                    "inputs": editor.inputs,
                    "roles": editor.roles,
                    "actions": editor.actions,
                    "output": controller.output_summary(),
                })
            })
            .collect();
        Ok(serde_json::json!({"status": "ok", "controllers": controllers}).to_string())
    })();
    result_string(&mut env, result)
}

/// Validates and persists setting values exactly as the desktop's PUT
/// endpoint does, applies them in-process, and returns the repaint plan.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_controllerApplySettings(
    mut env: JNIEnv,
    _class: JClass,
    store_root: JString,
    controller_id: JString,
    values_json: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let store_root: String = env
            .get_string(&store_root)
            .context("reading controller store root")?
            .into();
        let store_root = PathBuf::from(store_root);
        let controller_id: String = env
            .get_string(&controller_id)
            .context("reading controller id")?
            .into();
        let values_json: String = env
            .get_string(&values_json)
            .context("reading setting values")?
            .into();
        let requested: BTreeMap<String, String> =
            serde_json::from_str(&values_json).context("parsing setting values")?;
        let installed = rackforge_controller_package::PackageStore::new(&store_root)
            .resolve(&controller_id)
            .map_err(|error| anyhow::anyhow!("resolving controller: {error}"))?;
        let manifest = installed.package.manifest();
        let mut values = read_controller_store_settings(&store_root, &controller_id);
        for (id, value) in &requested {
            let setting = manifest
                .settings
                .iter()
                .find(|setting| &setting.id == id)
                .with_context(|| format!("this controller declares no setting {id:?}"))?;
            setting.validate_value(value).map_err(anyhow::Error::msg)?;
            values.insert(id.clone(), value.clone());
        }
        let path = controller_store_settings_path(&store_root, &controller_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = values
            .iter()
            .map(|(key, value)| {
                format!(
                    "{key} = {value:?}
"
                )
            })
            .collect::<String>();
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, body)?;
        fs::rename(&temporary, &path)?;
        apply_controller_settings_in_process(&values);
        let plan = controller_plan_value(keylab_protocol::ambient_repaint_messages())?;
        Ok(serde_json::json!({"status": "ok", "plan": plan}).to_string())
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
pub extern "system" fn Java_org_rackforge_android_MainActivity_ensurePerformanceLibrary(
    mut env: JNIEnv,
    _class: JClass,
    data_root: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let data_root = PathBuf::from(java_string(&mut env, data_root)?);
        ensure_performance_menu(&data_root)
    })();
    match result {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

/// LIVE's library requests -- the snapshot, and an edit -- answered from the
/// repository in the data root, as the Pi and the desktop answer them.
/// Returns the response's JSON; a refused edit is an `error` response, never
/// an exception, so the surface waiting on it hears why.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_performanceCommand(
    mut env: JNIEnv,
    _class: JClass,
    data_root: JString,
    request_json: JString,
    package_roots_json: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let data_root = PathBuf::from(java_string(&mut env, data_root)?);
        let request_json = java_string(&mut env, request_json)?;
        let package_roots: BTreeMap<String, String> =
            serde_json::from_str(&java_string(&mut env, package_roots_json)?).unwrap_or_default();
        let response = android_performance_command(&data_root, &request_json, &package_roots)
            .unwrap_or_else(|error| ControlResponse::Error {
                code: ControlErrorCode::Rejected,
                message: format!("{error:#}"),
                current_revision: None,
            });
        Ok(serde_json::to_string(&response)?)
    })();
    result_string(&mut env, result)
}

fn android_performance_command(
    data_root: &Path,
    request_json: &str,
    package_roots: &BTreeMap<String, String>,
) -> Result<ControlResponse> {
    ensure_performance_menu(data_root)?;
    let request: ControlRequest =
        serde_json::from_str(request_json).context("reading the LIVE library request")?;
    match request {
        ControlRequest::PerformanceSnapshot => {
            let guard = performance()
                .lock()
                .map_err(|_| anyhow::anyhow!("performance library lock poisoned"))?;
            let state = guard.as_ref().context("the LIVE library is not loaded")?;
            Ok(ControlResponse::PerformanceSnapshot {
                snapshot: Box::new(performance_snapshot(state)),
            })
        }
        ControlRequest::EditPerformance {
            expected_revision,
            edit,
        } => {
            expected_revision.validate()?;
            // Plugin work happens before the library is locked.
            let edit = materialize_rack_edit(edit, data_root, package_roots)?;
            let snapshot = {
                let mut guard = performance()
                    .lock()
                    .map_err(|_| anyhow::anyhow!("performance library lock poisoned"))?;
                let state = guard.as_mut().context("the LIVE library is not loaded")?;
                state
                    .repository
                    .apply_edit(&expected_revision, edit, &mut state.live)?;
                performance_snapshot(state)
            };
            controller_menu()
                .lock()
                .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))?
                .menu
                .sync_performance_snapshot(snapshot.clone());
            Ok(ControlResponse::PerformanceEdited {
                snapshot: Box::new(snapshot),
            })
        }
        _ => bail!("not a LIVE library request"),
    }
}

/// LIVE's navigation on Android, which plays one voice: the Activity asks
/// for a plan (the Rack a location resolves to and its Slots), switches the
/// voice to the Rack's instrument, and commits only once it sounds -- so the
/// library never says a Rack is loaded that is not.
///
/// `{"kind": "plan", "location"}` -> `{rack_id, rack_name, slots: [...]}`
/// `{"kind": "commit", "location"}` -> the performance snapshot
/// `{"kind": "browse", "mode"}` / `{"kind": "deactivate"}` -> the snapshot
/// `{"kind": "state"}` -> the LIVE state the session snapshot carries
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_liveCommand(
    mut env: JNIEnv,
    _class: JClass,
    data_root: JString,
    command_json: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let data_root = PathBuf::from(java_string(&mut env, data_root)?);
        let command: serde_json::Value =
            serde_json::from_str(&java_string(&mut env, command_json)?)
                .context("reading the LIVE command")?;
        Ok(android_live_command(&data_root, &command)?.to_string())
    })();
    result_string(&mut env, result)
}

fn android_live_command(
    data_root: &Path,
    command: &serde_json::Value,
) -> Result<serde_json::Value> {
    ensure_performance_menu(data_root)?;
    let location = || -> Result<rackforge_performance_api::LiveLocation> {
        serde_json::from_value(
            command
                .get("location")
                .cloned()
                .context("the LIVE command names no location")?,
        )
        .context("reading the LIVE location")
    };
    let kind = command
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .context("the LIVE command has no kind")?;
    // A Rack is built before anything is locked: its plugins load and
    // activate, which takes a moment, and audio keeps playing meanwhile.
    let package_roots = || -> Result<BTreeMap<String, String>> {
        serde_json::from_value(
            command
                .get("package_roots")
                .cloned()
                .context("the LIVE command names no installed packages")?,
        )
        .context("reading the installed packages")
    };
    let put_on_stage = |rack: AndroidRack| -> Result<()> {
        let retired = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
            .as_mut()
            .context("RackForge's audio engine is not running")?
            .install_rack(rack)?;
        drop(retired);
        Ok(())
    };
    let mut loaded = None;
    match kind {
        "load" => {
            let (rack, summary) = build_android_rack(data_root, &location()?, &package_roots()?)?;
            put_on_stage(rack)?;
            println!("LIVE_RACK_LOADED summary={summary}");
            loaded = Some(summary);
        }
        // A Rack being edited, heard as it stands. It is not the loaded
        // Rack: LIVE's record is left as it was, and leaving the editor
        // loads what was playing again.
        "preview" => {
            let draft: rackforge_performance_api::RackDefinition = serde_json::from_value(
                command
                    .get("rack")
                    .cloned()
                    .context("the preview names no Rack")?,
            )
            .context("reading the Rack to preview")?;
            let (rack, summary) = build_android_preview_rack(data_root, draft, &package_roots()?)?;
            put_on_stage(rack)?;
            println!("LIVE_RACK_PREVIEWED summary={summary}");
            return Ok(summary);
        }
        "deactivate" => drop_android_rack()?,
        _ => {}
    }
    let mut guard = performance()
        .lock()
        .map_err(|_| anyhow::anyhow!("performance library lock poisoned"))?;
    let state = guard.as_mut().context("the LIVE library is not loaded")?;
    let snapshot = match kind {
        "state" => return Ok(serde_json::to_value(&state.live)?),
        // The Rack is on stage; LIVE records it only now, so a Rack that
        // did not build is never shown as playing.
        "load" => {
            let location = location()?;
            let rack = state
                .repository
                .library()
                .resolve_playable(&location)
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            state.live.activate(location, rack.id.clone());
            performance_snapshot(state)
        }
        "browse" => {
            state.live.mode = serde_json::from_value(
                command
                    .get("mode")
                    .cloned()
                    .context("the browse command names no mode")?,
            )
            .context("reading the LIVE browse mode")?;
            performance_snapshot(state)
        }
        "deactivate" => {
            state.live.deactivate();
            performance_snapshot(state)
        }
        other => bail!("unknown LIVE command {other:?}"),
    };
    drop(guard);
    controller_menu()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))?
        .menu
        .sync_performance_snapshot(snapshot.clone());
    if let Some(summary) = loaded {
        return Ok(summary);
    }
    Ok(serde_json::to_value(snapshot)?)
}

/// A Rack saved with a Slot that has no state yet gets one made from its
/// plugin's package -- its first program, or the program the Slot names --
/// as the Pi makes it from the running instance.
fn materialize_rack_edit(
    edit: rackforge_performance_api::PerformanceEdit,
    data_root: &Path,
    package_roots: &BTreeMap<String, String>,
) -> Result<rackforge_performance_api::PerformanceEdit> {
    let rackforge_performance_api::PerformanceEdit::PutRack { mut rack } = edit else {
        return Ok(edit);
    };
    if rack.slots.iter().all(|slot| slot.state.is_some()) {
        return Ok(rackforge_performance_api::PerformanceEdit::PutRack { rack });
    }
    // The active plugin's runtime when there is one; a Rack is saved just
    // the same with no plugin active.
    let context = engine()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
        .as_ref()
        .map_or_else(
            || AndroidIsolatedStateContext::without_engine(data_root),
            AndroidEngine::isolated_state_context,
        );
    for slot in rack.slots.iter_mut().filter(|slot| slot.state.is_none()) {
        let package_root = package_roots
            .get(&slot.plugin_id)
            .with_context(|| format!("plugin {} is not installed", slot.plugin_id))?;
        let state = materialize_isolated_plugin_state(
            &context,
            &serde_json::json!({
                "plugin_id": slot.plugin_id,
                "package_root": package_root,
                "sound_id": slot.legacy_program_id,
            }),
        )
        .with_context(|| format!("making the state of Rack Slot {}", slot.id))?;
        slot.state = Some(serde_json::from_value(state)?);
    }
    Ok(rackforge_performance_api::PerformanceEdit::PutRack { rack })
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
        .and_then(|mut controller| {
            controller_plan_json(Ok(
                controller.render_plan(SurfaceUpdatePriority::Background)?
            ))
        });
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabInvalidateDisplay(
    _env: JNIEnv,
    _class: JClass,
) {
    if let Ok(mut controller) = controller_menu().lock() {
        controller.compositor.invalidate_delivery();
    }
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

/// The last parameter touch the KeyLab's header showed.
static PARAMETER_TOUCH_SEEN: AtomicU64 = AtomicU64::new(0);

/// The header for what a control last did to a parameter, if anything new:
/// its name and value in its own units, or where it stands and which way to
/// move while the control has not picked it up. Java asks just after it
/// forwards a control change, once the render thread has applied the link.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_keyLabParameterTouch(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let mut seen = PARAMETER_TOUCH_SEEN.load(Ordering::Relaxed);
    let Some(touch) = rackforge_core::parameter_touch::PARAMETER_TOUCHES.latest(&mut seen) else {
        return ptr::null_mut();
    };
    PARAMETER_TOUCH_SEEN.store(seen, Ordering::Relaxed);
    let header = engine().lock().ok().and_then(|guard| {
        guard
            .as_ref()
            .and_then(|engine| engine.parameter_touch_header(&touch))
    });
    let Some(header) = header else {
        return ptr::null_mut();
    };
    let result = controller_menu()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))
        .and_then(|mut controller| controller.render_transient_header(&header));
    result_string(&mut env, result)
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
    let profile = keylab_essential_mk3::controller::package_profile();
    if let Some(input) = profile
        .semantic_profile
        .as_ref()
        .and_then(|profile| rackforge_parameter_input(profile, &message))
    {
        let current_pan = MasterPan::new(MASTER_PAN_VALUE.load(Ordering::Relaxed) as i16)
            .unwrap_or(MasterPan::CENTER);
        let result = controller_parameter_mapper()
            .lock()
            .map_err(|_| anyhow::anyhow!("RackForge parameter mapper lock poisoned"))
            .map(|mut mapper| {
                mapper.apply(input, current_pan).inspect(|&parameter| {
                    apply_rackforge_parameter(parameter);
                })
            })
            .and_then(|parameter| match parameter {
                Some(parameter) => controller_menu()
                    .lock()
                    .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))
                    .and_then(|mut controller| controller.render_rackforge_parameter(parameter)),
                None => Ok(serde_json::json!({
                    "plan": [],
                    "command": null,
                })
                .to_string()),
            });
        return result_string(&mut env, result);
    }
    // A default-mapped control plays on to its link; the header names the
    // parameter it moved once the link has run (keyLabParameterTouch), not
    // the role's fixed label and the raw CC.
    // Nothing to draw and nothing consumed, so no answer at all: an empty
    // one would still cancel the pending restore of a header on screen.
    if profile
        .semantic_profile
        .as_ref()
        .and_then(|profile| semantic_control_input(profile, &message))
        .is_some()
    {
        return ptr::null_mut();
    }
    let Some(event) = keylab_protocol::parse_input(&message) else {
        return ptr::null_mut();
    };
    let result = match event {
        keylab_protocol::ControllerEvent::Surface { input, phase } => controller_menu()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller menu lock poisoned"))
            .and_then(|mut controller| controller.handle_surface(input, phase)),
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
    PLUGIN_INSTALL_CANCELLED.store(false, Ordering::Release);
    let result = (|| -> Result<String> {
        let archive_path = PathBuf::from(java_string(&mut env, archive_path)?);
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        let bytes = std::fs::read(&archive_path)
            .with_context(|| format!("reading selected plugin {}", archive_path.display()))?;
        let installed =
            install_local_archive_cancellable(&store_root, &bytes, &PLUGIN_INSTALL_CANCELLED)
                .context("validating and installing the portable plugin")?;
        let package = PluginPackage::open(&installed.path)
            .with_context(|| format!("opening installed plugin {}", installed.path.display()))?;
        let mut descriptor = package_descriptor(&package, false);
        // A layout the new package carries reaches the keyboards' maps.
        if let Err(error) = reoffer_android_factory_maps() {
            eprintln!("FACTORY_CONTROLLER_MAPS_FAILED error={error:#}");
        }
        descriptor["already_installed"] = installed.already_installed.into();
        descriptor["artifact_sha256"] = installed.record.artifact_sha256.into();
        Ok(descriptor.to_string())
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_cancelPluginInstall(
    _env: JNIEnv,
    _class: JClass,
) {
    PLUGIN_INSTALL_CANCELLED.store(true, Ordering::Release);
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
        if let Err(error) = reoffer_android_factory_maps() {
            eprintln!("FACTORY_CONTROLLER_MAPS_FAILED error={error:#}");
        }
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
        if let Err(error) = ensure_performance_menu(&data_root) {
            eprintln!("PERFORMANCE_LIBRARY_UNAVAILABLE {error:#}");
        }
        if let Some(current) = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
            .as_ref()
        {
            current.live_parameter_writer_handle.flush();
        }
        let mut candidate = AndroidEngine::open_package(&package_root, data_root.clone())?;
        candidate.recompile_parameter_links()?;
        set_plugin_enabled(
            packages_root.parent().unwrap_or(&packages_root),
            &candidate.plugin_id,
            true,
        )?;
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
pub extern "system" fn Java_org_rackforge_android_MainActivity_setInstalledPluginEnabled(
    mut env: JNIEnv,
    _class: JClass,
    plugin_id: JString,
    store_root: JString,
    enabled: jboolean,
) -> jboolean {
    let result = (|| -> Result<()> {
        let plugin_id = java_string(&mut env, plugin_id)?;
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        set_plugin_enabled(&store_root, &plugin_id, enabled == JNI_TRUE)?;
        if let Err(error) = reoffer_android_factory_maps() {
            eprintln!("FACTORY_CONTROLLER_MAPS_FAILED error={error:#}");
        }
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
pub extern "system" fn Java_org_rackforge_android_MainActivity_deactivateInstalledPlugin(
    mut env: JNIEnv,
    _class: JClass,
    plugin_id: JString,
    store_root: JString,
) -> jint {
    let result = (|| -> Result<i32> {
        let plugin_id = java_string(&mut env, plugin_id)?;
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        let was_current = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
            .as_ref()
            .is_some_and(|engine| engine.plugin_id == plugin_id);
        set_plugin_enabled(&store_root, &plugin_id, false)?;
        if was_current {
            midi_queue()
                .lock()
                .map_err(|_| anyhow::anyhow!("MIDI queue lock poisoned"))?
                .clear();
            *engine()
                .lock()
                .map_err(|_| anyhow::anyhow!("engine lock poisoned"))? = None;
        }
        Ok(if was_current { 1 } else { 2 })
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
        enqueue_midi(VIRTUAL_MIDI_SOURCE_KEY, &bytes[..length as usize]);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_sendMidiMessageFromSource(
    _env: JNIEnv,
    _class: JClass,
    source_key: jint,
    status: jint,
    data_1: jint,
    data_2: jint,
    length: jint,
) {
    let bytes = [status as u8, data_1 as u8, data_2 as u8];
    if source_key > 0 && (1..=3).contains(&length) {
        let source = MidiSourceKey::new(source_key as u32);
        let message = &bytes[..length as usize];
        record_midi_activity(source, message);
        if apply_reserved_host_input(source, message) {
            return;
        }
        apply_declarative_rackforge_parameter(source, message);
        // MIDI Start, Continue and Stop are a controller's buttons, never an
        // instrument's input.
        if MidiPacket::transport_realtime(0, message).is_some() {
            return;
        }
        enqueue_midi(source, message);
    }
}

/// A declarative controller's reserved controls and buttons, as the other
/// hosts read them: the master level and pan move, and neither they nor a
/// host action's button reach an instrument. Android has no sequencer yet,
/// so a transport or lane button is taken and said so, not played as a note.
fn apply_reserved_host_input(source: MidiSourceKey, message: &[u8]) -> bool {
    let Some(source_id) = midi_sources().lock().ok().and_then(|sources| {
        sources
            .descriptor(source)
            .map(|descriptor| descriptor.id.clone())
    }) else {
        return false;
    };
    let (control, action) = {
        let Ok(controllers) = midi_controllers().lock() else {
            return false;
        };
        let Some(controller) = controllers.get(&source_id) else {
            return false;
        };
        (
            controller
                .host_controls
                .iter()
                .find_map(|binding| Some((binding.target, binding.midi_cc.value(message)?))),
            controller
                .host_actions
                .iter()
                .find_map(|binding| Some((binding.target, binding.phase(message)?))),
        )
    };
    if let Some((target, value)) = control {
        apply_rackforge_parameter(match target {
            HostControlTarget::MasterLevel => {
                RackForgeParameterValue::MasterLevel(MasterLevel::from_midi(value))
            }
            HostControlTarget::MasterPan => {
                RackForgeParameterValue::MasterPan(MasterPan::from_midi_with_center_snap(value))
            }
        });
        return true;
    }
    if let Some((target, phase)) = action {
        if phase == ButtonPhase::Press {
            eprintln!("ANDROID_HOST_ACTION_UNAVAILABLE target={target:?} reason=no-sequencer");
        }
        return true;
    }
    false
}

/// What came in, for the Controllers section's lit inputs.
fn record_midi_activity(source: MidiSourceKey, message: &[u8]) {
    let Some(descriptor) = midi_sources()
        .lock()
        .ok()
        .and_then(|sources| sources.descriptor(source).cloned())
    else {
        return;
    };
    if let Ok(mut activity) = midi_activity().lock() {
        activity.record(&descriptor, message);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_resetMidiSources(
    mut env: JNIEnv,
    _class: JClass,
) {
    let result = (|| -> Result<()> {
        *midi_sources()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI source registry lock poisoned"))? =
            MidiSourceRegistry::default();
        midi_controllers()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI controller lock poisoned"))?
            .clear();
        NEXT_MIDI_SOURCE_KEY.store(1, Ordering::Relaxed);
        recompile_engine_parameter_links()
    })();
    if let Err(error) = result {
        report(&mut env, error);
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_registerMidiSource(
    mut env: JNIEnv,
    _class: JClass,
    source_id: JString,
    display_name: JString,
    primary: jboolean,
    controller_id: JString,
) -> jint {
    let result = (|| -> Result<u32> {
        let controller_id = java_string(&mut env, controller_id)?;
        let descriptor = MidiSourceDescriptor {
            id: MidiSourceId::new(java_string(&mut env, source_id)?)?,
            name: java_string(&mut env, display_name)?,
            primary: primary == JNI_TRUE,
        };
        let source_id = descriptor.id.clone();
        let endpoint_name = descriptor.name.clone();
        let mut sources = midi_sources()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI source registry lock poisoned"))?;
        let key = if let Some(existing) = sources.resolve_optional(&descriptor.id) {
            existing
        } else {
            let key = MidiSourceKey::new(NEXT_MIDI_SOURCE_KEY.fetch_add(1, Ordering::Relaxed));
            sources.register(key, descriptor)?;
            key
        };
        drop(sources);
        {
            let mut controllers = midi_controllers()
                .lock()
                .map_err(|_| anyhow::anyhow!("MIDI controller lock poisoned"))?;
            // Registered again while connected: what the device said and
            // what was sent to it still hold.
            if let Some(existing) = controllers.get_mut(&source_id) {
                existing.hint = controller_id;
                existing.endpoint_name = endpoint_name;
                existing.resolve();
            } else {
                controllers.insert(
                    source_id,
                    AndroidMidiController::new(controller_id, endpoint_name),
                );
            }
        }
        recompile_engine_parameter_links()?;
        Ok(key.get())
    })();
    match result {
        Ok(key) => key as jint,
        Err(error) => {
            report(&mut env, error);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_replaceParameterLinks(
    mut env: JNIEnv,
    _class: JClass,
    links_json: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let links: Vec<ParameterLink> = serde_json::from_str(&java_string(&mut env, links_json)?)
            .context("parsing Android MIDI parameter links")?;
        let mut guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        let engine = guard
            .as_mut()
            .context("RackForge engine is not initialized")?;
        engine.replace_parameter_links(links)
    })();
    match result {
        Ok(()) => JNI_TRUE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

/// The session's controller requests -- the player's maps, the inputs that
/// came in, and the controllers the player makes -- answered as the desktop
/// and the Pi answer them. Returns the response's JSON; a refused request is
/// an `error` response, never an exception, so the UI hears every answer.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_controllerSessionCommand(
    mut env: JNIEnv,
    _class: JClass,
    data_root: JString,
    request_json: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let data_root = PathBuf::from(java_string(&mut env, data_root)?);
        let request_json = java_string(&mut env, request_json)?;
        let response = match controller_session_command(&data_root, &request_json) {
            Ok(response) => response,
            Err(error) => ControlResponse::Error {
                code: ControlErrorCode::InvalidRequest,
                message: format!("{error:#}"),
                current_revision: None,
            },
        };
        Ok(serde_json::to_string(&response)?)
    })();
    result_string(&mut env, result)
}

/// Loads the player's controller maps before the first plugin compiles its
/// links, so a mapped control works from the first note.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_loadControllerMaps(
    mut env: JNIEnv,
    _class: JClass,
    data_root: JString,
    plugin_store_root: JString,
    controller_store_root: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let data_root = PathBuf::from(java_string(&mut env, data_root)?);
        {
            let mut maps = controller_maps()
                .lock()
                .map_err(|_| anyhow::anyhow!("controller map lock poisoned"))?;
            maps.plugin_store_root = Some(PathBuf::from(java_string(&mut env, plugin_store_root)?));
            maps.controller_store_root =
                Some(PathBuf::from(java_string(&mut env, controller_store_root)?));
        }
        if ensure_controller_maps(&data_root)? {
            recompile_engine_parameter_links()?;
        }
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

fn controller_session_command(data_root: &Path, request_json: &str) -> Result<ControlResponse> {
    if ensure_controller_maps(data_root)? {
        recompile_engine_parameter_links()?;
    }
    let request: ControlRequest =
        serde_json::from_str(request_json).context("reading the controller request")?;
    Ok(match request {
        ControlRequest::ControllerMaps => android_controller_maps_response()?,
        ControlRequest::MidiActivity { after } => {
            let (cursor, events) = midi_activity()
                .lock()
                .map_err(|_| anyhow::anyhow!("MIDI activity lock poisoned"))?
                .since(after);
            ControlResponse::MidiActivity { cursor, events }
        }
        ControlRequest::SaveControllerMap { map } => {
            let map = save_android_controller_map(*map)?;
            ControlResponse::ControllerMapSaved { map: Box::new(map) }
        }
        ControlRequest::SetControllerTakeover { takeover } => {
            set_android_controller_takeover(takeover)?;
            ControlResponse::ControllerTakeoverSet { takeover }
        }
        // What a control last moved, for the window at the top of the page:
        // `after` zero only learns where the touches stand.
        ControlRequest::ParameterTouch { after } => {
            use rackforge_core::parameter_touch::PARAMETER_TOUCHES;
            let mut sequence = after;
            let touch = if after == 0 {
                sequence = PARAMETER_TOUCHES.current_sequence();
                None
            } else {
                PARAMETER_TOUCHES.latest(&mut sequence).and_then(|touch| {
                    engine()
                        .lock()
                        .ok()?
                        .as_ref()?
                        .parameter_touch_report(&touch)
                })
            };
            ControlResponse::ParameterTouched {
                sequence,
                touch: touch.map(Box::new),
            }
        }
        ControlRequest::ImportControllerMap { file } => {
            file.validate()?;
            let map = save_android_controller_map(file.map)?;
            ControlResponse::ControllerMapImported { map: Box::new(map) }
        }
        ControlRequest::ExportControllerMap { controller_id } => {
            let maps = controller_maps()
                .lock()
                .map_err(|_| anyhow::anyhow!("controller map lock poisoned"))?;
            let map = maps
                .maps
                .get(&controller_id)
                .with_context(|| format!("no map is stored for controller {controller_id}"))?;
            let exported_unix_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_millis() as u64);
            let (file_name, file) = export_rfmap(
                map,
                &format!("RackForge for Android {}", env!("CARGO_PKG_VERSION")),
                exported_unix_ms,
            );
            ControlResponse::ControllerMapExported {
                file_name,
                file: Box::new(file),
            }
        }
        ControlRequest::SaveUserController { controller } => {
            let root = controller_store_root().context("the controller store is not ready")?;
            let installed = rackforge_controller_package::PackageStore::new(root)
                .save_user_controller(&controller)
                .map_err(|error| anyhow::anyhow!("Could not save the controller: {error}"))?;
            // Attached at once to the input it was made on.
            reresolve_midi_controllers()?;
            ControlResponse::UserControllerSaved {
                controller_id: installed.record.id,
                version: installed.record.version,
            }
        }
        _ => bail!("not a controller request"),
    })
}

/// The controllers on connected inputs, and every stored map.
fn android_controller_maps_response() -> Result<ControlResponse> {
    let sources = midi_sources()
        .lock()
        .map_err(|_| anyhow::anyhow!("MIDI source registry lock poisoned"))?;
    let controllers = midi_controllers()
        .lock()
        .map_err(|_| anyhow::anyhow!("MIDI controller lock poisoned"))?;
    let mut registered = BTreeMap::<String, RegisteredController>::new();
    for (source_id, controller) in controllers.iter() {
        let Some(controller_id) = &controller.controller_id else {
            continue;
        };
        let source = sources
            .resolve_optional(source_id)
            .and_then(|key| sources.descriptor(key).cloned());
        // One entry per controller: a device with several ports lists the
        // first, as the desktop does.
        registered
            .entry(controller_id.clone())
            .or_insert(RegisteredController {
                controller_id: controller_id.clone(),
                connected: source.is_some(),
                source,
                identified: controller.identified,
            });
    }
    let maps = controller_maps()
        .lock()
        .map_err(|_| anyhow::anyhow!("controller map lock poisoned"))?;
    Ok(ControlResponse::ControllerMaps {
        controllers: registered.into_values().collect(),
        maps: maps.maps.values().cloned().collect(),
        takeover: maps.takeover,
        factory_untouched: maps
            .store
            .as_ref()
            .map(ControllerMapStore::untouched_factory_maps)
            .unwrap_or_default(),
    })
}

/// Stores a controller's whole map and applies it at once, as a learnt link
/// is. An empty map removes it.
fn save_android_controller_map(map: ControllerMap) -> Result<ControllerMap> {
    map.validate()?;
    {
        let mut maps = controller_maps()
            .lock()
            .map_err(|_| anyhow::anyhow!("controller map lock poisoned"))?;
        maps.store
            .as_ref()
            .context("controller maps are not loaded")?
            .save(&map)
            .map_err(|error| anyhow::anyhow!("Could not store the controller map: {error:#}"))?;
        if map.is_empty() {
            maps.maps.remove(&map.controller_id);
        } else {
            maps.maps.insert(map.controller_id.clone(), map.clone());
        }
    }
    recompile_engine_parameter_links()?;
    Ok(map)
}

/// A device answered the Identity Request on this source: the package is
/// resolved again with its answer. Returns whether the source's package
/// changed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_identifyMidiSource(
    mut env: JNIEnv,
    _class: JClass,
    source_key: jint,
    reply: JByteArray,
) -> jboolean {
    let result = (|| -> Result<bool> {
        let bytes = env.convert_byte_array(&reply)?;
        let Some(identity) = rackforge_controller_package::IdentityReply::parse(&bytes) else {
            return Ok(false);
        };
        let Some(source_id) = midi_sources()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI source registry lock poisoned"))?
            .descriptor(MidiSourceKey::new(source_key as u32))
            .map(|descriptor| descriptor.id.clone())
        else {
            return Ok(false);
        };
        let changed = {
            let mut controllers = midi_controllers()
                .lock()
                .map_err(|_| anyhow::anyhow!("MIDI controller lock poisoned"))?;
            let Some(controller) = controllers.get_mut(&source_id) else {
                return Ok(false);
            };
            eprintln!(
                "ANDROID_MIDI_IDENTITY source={source_id} manufacturer={:02X?} family={:#06x} model={:#06x}",
                identity.manufacturer, identity.family, identity.model
            );
            controller.identity = Some(identity);
            let before = controller.controller_id.clone();
            controller.resolve();
            before != controller.controller_id
        };
        if changed {
            recompile_engine_parameter_links()?;
        }
        Ok(changed)
    })();
    match result {
        Ok(true) => JNI_TRUE,
        Ok(false) => JNI_FALSE,
        Err(error) => {
            report(&mut env, error);
            JNI_FALSE
        }
    }
}

/// The messages the source's package sends when its controller connects, as
/// a controller plan, once per connection: empty when there are none, the
/// player has not allowed them, or they were sent.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_midiSourceConnectPlan(
    mut env: JNIEnv,
    _class: JClass,
    source_key: jint,
) -> jstring {
    let result = (|| -> Result<String> {
        let source_id = midi_sources()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI source registry lock poisoned"))?
            .descriptor(MidiSourceKey::new(source_key as u32))
            .map(|descriptor| descriptor.id.clone());
        let mut controllers = midi_controllers()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI controller lock poisoned"))?;
        let Some(controller) = source_id.and_then(|id| controllers.get_mut(&id)) else {
            return Ok("[]".into());
        };
        if controller.on_connect_sent
            || (controller.on_connect.is_empty() && controller.setup_messages.is_empty())
        {
            return Ok("[]".into());
        }
        controller.on_connect_sent = true;
        // The controller's own port first, then its setup output: the DAW
        // port some controllers change mode through. Java finds that port
        // with `midiSetupPort`.
        let plan = controller
            .on_connect
            .iter()
            .map(|bytes| (bytes, "input"))
            .chain(
                controller
                    .setup_messages
                    .iter()
                    .map(|bytes| (bytes, "setup")),
            )
            .map(|(bytes, port)| {
                serde_json::json!({
                    "bytes": bytes,
                    "port": port,
                    // A device may need a moment between mode changes.
                    "settle_after_ms": 20,
                })
            })
            .collect::<Vec<_>>();
        eprintln!(
            "ANDROID_CONTROLLER_ON_CONNECT controller={:?} messages={}",
            controller.controller_id,
            plan.len()
        );
        Ok(serde_json::Value::Array(plan).to_string())
    })();
    result_string(&mut env, result)
}

/// Which of a device's MIDI inputs is the setup output of the controller on
/// `source_key`, among the names Java lists them by: the index, or -1 when
/// the package names none or no single port matches. The same choice the
/// other hosts make, by the package's `setup_output` endpoint.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_midiSetupPort(
    mut env: JNIEnv,
    _class: JClass,
    source_key: jint,
    port_names_json: JString,
) -> jint {
    let result = (|| -> Result<i32> {
        let names: Vec<String> = serde_json::from_str(&java_string(&mut env, port_names_json)?)?;
        let source_id = midi_sources()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI source registry lock poisoned"))?
            .descriptor(MidiSourceKey::new(source_key as u32))
            .map(|descriptor| descriptor.id.clone());
        let controllers = midi_controllers()
            .lock()
            .map_err(|_| anyhow::anyhow!("MIDI controller lock poisoned"))?;
        let Some(controller) = source_id.and_then(|id| controllers.get(&id)) else {
            return Ok(-1);
        };
        let Some(matcher) = &controller.setup_output else {
            return Ok(-1);
        };
        let chosen = rackforge_controller_package::setup_output_port(
            matcher,
            &controller.endpoint_name,
            &names,
        );
        Ok(chosen
            .and_then(|name| names.iter().position(|candidate| *candidate == name))
            .map_or(-1, |index| index as i32))
    })();
    match result {
        Ok(index) => index,
        Err(error) => {
            report(&mut env, error);
            -1
        }
    }
}

/// A device unplugged: what its controller was sent goes again when it
/// comes back, as a new connection.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_midiSourceDisconnected(
    _env: JNIEnv,
    _class: JClass,
    source_key: jint,
) {
    let source_id = midi_sources().lock().ok().and_then(|sources| {
        sources
            .descriptor(MidiSourceKey::new(source_key as u32))
            .map(|descriptor| descriptor.id.clone())
    });
    if let Some(source_id) = source_id
        && let Ok(mut controllers) = midi_controllers().lock()
        && let Some(controller) = controllers.get_mut(&source_id)
    {
        controller.on_connect_sent = false;
        // The device may come back as another model's firmware, or answer
        // the Identity Request differently: it is asked again.
        controller.identity = None;
        controller.resolve();
    }
}

/// Records whether the player lets a package send its on_connect messages,
/// and resolves the connected sources again so an allowed package's
/// messages go out with the next connect plan.
#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_controllerAllowOutput(
    mut env: JNIEnv,
    _class: JClass,
    store_root: JString,
    controller_id: JString,
    allow: jboolean,
) -> jstring {
    let result = (|| -> Result<String> {
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        let controller_id = java_string(&mut env, controller_id)?;
        let installed = rackforge_controller_package::PackageStore::new(store_root)
            .allow_output(&controller_id, allow == JNI_TRUE)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        reresolve_midi_controllers()?;
        Ok(serde_json::json!({
            "status": "ok",
            "id": installed.record.id,
            "output": installed.output_summary(),
        })
        .to_string())
    })();
    result_string(&mut env, result)
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
pub extern "system" fn Java_org_rackforge_android_MainActivity_outputMeterSnapshot(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = serde_json::to_string(&OUTPUT_METER.take())
        .context("serializing Android output meter")
        .and_then(|value| Ok(env.new_string(value)?.into_raw()));
    match result {
        Ok(value) => value,
        Err(error) => {
            report(&mut env, error);
            ptr::null_mut()
        }
    }
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
pub extern "system" fn Java_org_rackforge_android_MainActivity_setPlayChain(
    mut env: JNIEnv,
    _class: JClass,
    chain_json: JString,
    store_root: JString,
) -> jstring {
    let result = (|| -> Result<String> {
        let chain_json = java_string(&mut env, chain_json)?;
        let store_root = PathBuf::from(java_string(&mut env, store_root)?);
        let effects: Vec<PlayChainEffect> = if chain_json.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(&chain_json).context("reading the PLAY chain")?
        };
        // Compiling a plugin's runtime takes far longer than an audio block,
        // so it happens before the engine is locked: what the audio thread
        // waits for is the instances, not the compiler.
        let data_root = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
            .as_ref()
            .map(|engine| engine.data_root.clone());
        if let Some(data_root) = data_root {
            for effect in &effects {
                if let Ok(package) = installed_package(&store_root, &effect.plugin_id) {
                    let _ = cached_isolated_plugin_runtime(&package, &data_root);
                }
            }
        }
        let mut guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        let engine = guard
            .as_mut()
            .context("RackForge engine is not initialized")?;
        engine.set_play_chain(&effects, &store_root)?;
        Ok(engine.play_chain_json().to_string())
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_playChain(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = (|| -> Result<String> {
        Ok(engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
            .as_ref()
            .map_or_else(
                || serde_json::json!({"effects": [], "instances": []}),
                AndroidEngine::play_chain_json,
            )
            .to_string())
    })();
    result_string(&mut env, result)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_selectChainSound(
    mut env: JNIEnv,
    _class: JClass,
    instance_id: JString,
    sound_id: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let instance_id = java_string(&mut env, instance_id)?;
        let sound_id = java_string(&mut env, sound_id)?;
        engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
            .as_mut()
            .context("RackForge engine is not initialized")?
            .select_chain_sound(&instance_id, &sound_id)
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
pub extern "system" fn Java_org_rackforge_android_MainActivity_restorePluginSound(
    mut env: JNIEnv,
    _class: JClass,
    sound_id: JString,
) -> jboolean {
    let result = (|| -> Result<()> {
        let sound_id = java_string(&mut env, sound_id)?;
        engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?
            .as_mut()
            .context("RackForge engine is not initialized")?
            .restore_sound(&sound_id)
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
        if matches!(
            method.as_str(),
            "materialize" | "catalog" | "plugin_state_parameters" | "set_plugin_state_parameter"
        ) {
            // State inspection creates a separate portable instance. Copy its immutable
            // context while holding ENGINE, then release the audio lock before Wasm
            // instantiation, state loading and hashing. With no plugin active a
            // Slot's plugin still comes from its package, given the data root.
            let context = {
                let guard = engine()
                    .lock()
                    .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
                match guard.as_ref() {
                    Some(engine) => engine.isolated_state_context(),
                    None => AndroidIsolatedStateContext::without_engine(Path::new(
                        params
                            .get("data_root")
                            .and_then(serde_json::Value::as_str)
                            .context("RackForge engine is not initialized")?,
                    )),
                }
            };
            return Ok(isolated_plugin_state_command(&context, &method, &params)?.to_string());
        }
        let mut guard = engine()
            .lock()
            .map_err(|_| anyhow::anyhow!("engine lock poisoned"))?;
        let engine = guard
            .as_mut()
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
                    current.parallel = AndroidEngine::rebuild_parallel_units(
                        current.runtime.0,
                        &current.resource_overrides,
                        &current.selected_sound_id,
                    );
                    current.process_faulted = false;
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
                    current.parallel = AndroidEngine::rebuild_parallel_units(
                        current.runtime.0,
                        &current.resource_overrides,
                        &current.selected_sound_id,
                    );
                    current.process_faulted = false;
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
        AUDIO_CALLBACK_OVERRUNS.store(0, Ordering::Relaxed);
        AUDIO_ENGINE_LOCK_MISSES.store(0, Ordering::Relaxed);
        AUDIO_RENDER_ERRORS.store(0, Ordering::Relaxed);
        AUDIO_NONFINITE_SAMPLES.store(0, Ordering::Relaxed);
        AUDIO_CLIPPED_SAMPLES.store(0, Ordering::Relaxed);
        AUDIO_RENDER_THREAD_PRIORITY_RESULT.store(0, Ordering::Relaxed);
        AUDIO_DROPOUT_RECOVERY.reset();
        if AUDIO_STREAM_RECOVERY.snapshot().health == AudioStreamHealth::Lost {
            AUDIO_STREAM_RECOVERY.mark_recovering();
        }
        let candidate = match NativeAudioOutput::open(device_id, latency_mode) {
            Ok(candidate) => candidate,
            Err(error) => {
                AUDIO_STREAM_RECOVERY.mark_lost();
                return Err(error);
            }
        };
        *audio()
            .lock()
            .map_err(|_| anyhow::anyhow!("audio lock poisoned"))? = Some(candidate);
        AUDIO_STREAM_RECOVERY.mark_healthy();
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
pub extern "system" fn Java_org_rackforge_android_MainActivity_setNativeMasterLevel(
    _env: JNIEnv,
    _class: JClass,
    level: jint,
) -> jboolean {
    let Ok(level) = u16::try_from(level) else {
        return JNI_FALSE;
    };
    let Ok(level) = MasterLevel::new(level) else {
        return JNI_FALSE;
    };
    apply_rackforge_parameter(RackForgeParameterValue::MasterLevel(level));
    JNI_TRUE
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_nativeMasterLevel(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    MASTER_LEVEL_VALUE.load(Ordering::Relaxed) as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_setNativeMasterPan(
    _env: JNIEnv,
    _class: JClass,
    pan: jint,
) -> jboolean {
    let Ok(pan) = i16::try_from(pan) else {
        return JNI_FALSE;
    };
    let Ok(pan) = MasterPan::new(pan) else {
        return JNI_FALSE;
    };
    apply_rackforge_parameter(RackForgeParameterValue::MasterPan(pan));
    JNI_TRUE
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_org_rackforge_android_MainActivity_nativeMasterPan(
    _env: JNIEnv,
    _class: JClass,
) -> jint {
    MASTER_PAN_VALUE.load(Ordering::Relaxed) as jint
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
