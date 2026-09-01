#[cfg(target_os = "linux")]
pub mod audio;
pub mod audio_reliability;
#[cfg(target_os = "linux")]
pub mod control;
pub mod default_instrument;
pub mod hosted;
pub mod isolated_state;
#[cfg(target_os = "linux")]
pub mod live;
#[cfg(any(target_os = "linux", test))]
mod live_midi_state;
pub mod live_parameter_state;
pub mod live_show;
#[cfg(not(target_arch = "wasm32"))]
pub mod loader;
/// Browsers cannot load native plugin binaries, so the wasm host compiles a
/// refusing stand-in with the same shape instead.
#[cfg(target_arch = "wasm32")]
#[path = "loader_unavailable.rs"]
pub mod loader;
pub mod midi2;
pub mod midi_hotplug;
pub mod midi_trace;
pub mod package;
#[cfg(not(target_arch = "wasm32"))]
pub mod parallel_render;
pub mod parameter_link;
pub mod performance;
pub mod rack_graph;
pub mod realtime;
pub mod session;
pub mod session_checkpoint;
pub mod ump;
pub use rackforge_startup as startup;
/// The sequencer engine rides the same rule: patterns into sample-accurate
/// MIDI on every platform, gated by nothing.
pub mod sequencer;
pub mod state_store;
pub mod storage;
/// Deliberately outside every platform gate: the clock is host infrastructure
/// like the render pool, and the browser, Android and desktop hosts advance
/// the same arithmetic the Linux LIVE host does.
pub mod transport;

pub use default_instrument::{DEFAULT_INSTRUMENT_ID, choose_opening_instrument};
pub use hosted::{LoadedPlugin, PluginInstance, unload_process_handlers};
pub use isolated_state::{
    IsolatedPluginStateEditor, plugin_parameters, set_plugin_parameter, validate_parameter_write,
    validate_state_reference,
};
pub use live_parameter_state::LiveParameterStateStore;
#[cfg(not(target_arch = "wasm32"))]
pub use live_parameter_state::{
    LiveParameterTarget, LiveParameterWriter, LiveParameterWriterHandle,
};
pub use package::{PluginPackage, platform_key};
pub use parameter_link::{
    CompiledParameterLink, ParameterLinkOutput, SemanticParameterLinkContext,
    compile_semantic_parameter_links,
};
pub use sequencer::{CompiledPattern, SequencerEngine, SequencerLane};
pub use state_store::{MAX_PLUGIN_STATE_BYTES, PluginStateStore};
pub use storage::{PluginDirectory, PluginStorage, RECOMMENDED_PROGRAM_SUFFIX};
pub use transport::{Transport, TransportBlock, TransportSnapshot};
