#[cfg(target_os = "linux")]
pub mod audio;
#[cfg(target_os = "linux")]
pub mod control;
pub mod hosted;
pub mod isolated_state;
#[cfg(target_os = "linux")]
pub mod live;
#[cfg(any(target_os = "linux", test))]
mod live_midi_state;
pub mod loader;
pub mod midi_hotplug;
pub mod package;
pub mod performance;
pub mod rack_graph;
pub mod realtime;
pub mod session;
pub mod session_checkpoint;
pub mod state_store;
pub mod storage;

pub use hosted::{LoadedPlugin, PluginInstance};
pub use isolated_state::{
    IsolatedPluginStateEditor, plugin_parameters, set_plugin_parameter, validate_parameter_write,
    validate_state_reference,
};
pub use package::{PluginPackage, platform_key};
pub use state_store::{MAX_PLUGIN_STATE_BYTES, PluginStateStore};
pub use storage::{PluginDirectory, PluginStorage, RECOMMENDED_PROGRAM_SUFFIX};
