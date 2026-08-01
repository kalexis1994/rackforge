//! Stable contracts shared by the RackForge host and native plugins.
//!
//! Rust types in [`manifest`] and [`parameter`] are serialized contracts.
//! The binary boundary in [`abi`] deliberately uses only C-compatible values.

pub mod abi;
pub mod manifest;
pub mod parameter;
pub mod preset;
pub mod program;
pub mod state;

pub use manifest::{
    ApiRequirement, Capability, MidiInputBus, MidiProgramChangePolicy, PluginKind, PluginManifest,
    PluginMidiContract, ResourceKind, ResourceRequirement, RuntimeDescriptor, WebSurface,
    WebSurfaceKind, WebUi,
};
pub use parameter::{
    EnumChoice, PageDescriptor, ParameterDescriptor, ParameterFlags, ParameterKind,
    ParameterSchema, SuggestedControl,
};
pub use preset::{BankDescriptor, PresetCatalog, PresetDescriptor};
pub use program::{
    PROGRAM_EDIT_SCHEMA_VERSION, PROGRAM_EDITOR_SCHEMA_VERSION, PROGRAM_SCHEMA_VERSION,
    PreparedProgram, ProgramDocument, ProgramEditRequest, ProgramEditorChoice, ProgramEditorField,
    ProgramEditorFieldKind, ProgramEditorPage, ProgramEditorValue, ProgramEditorView, ProgramError,
    ProgramFieldEditRequest, validate_plugin_identifier, validate_program_identifier,
};
pub use rackforge_midi_api::{MidiInputBusId, PluginChannelModel};
pub use rackforge_surface_api::{
    SURFACE_ACTIVATION_SCHEMA_VERSION, SurfaceActivationReason, SurfaceActivationRequest,
    SurfaceActivationResponse, SurfaceError, SurfaceMode,
};
pub use state::{
    HOST_PRESET_SCHEMA_VERSION, HostPreset, HostPresetSummary,
    PLUGIN_STATE_REFERENCE_SCHEMA_VERSION, PluginStateError, PluginStateReference,
    validate_preset_id, validate_preset_name,
};

pub const MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const PARAMETER_SCHEMA_VERSION: u32 = 1;
pub const PRESET_CATALOG_SCHEMA_VERSION: u32 = 1;
pub const RUNTIME_DESCRIPTOR_SCHEMA_VERSION: u32 = 1;
