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
    ApiRequirement, AudioBus, AudioBusLayout, BrandingAssetKind, Capability, MAIN_AUDIO_BUS_ID,
    MAX_PLUGIN_AUDIO_CHANNELS, MidiInputBus, MidiProgramChangePolicy, PluginAudioContract,
    PluginBranding, PluginKind, PluginManifest, PluginMidiContract, PortableAbi, PortableComponent,
    ResourceKind, ResourceRequirement, RuntimeDescriptor, WebSurface, WebSurfaceKind, WebUi,
};
#[cfg(feature = "package-validation")]
pub use manifest::{
    BrandingAssetError, BrandingAssetsError, validate_branding_asset, validate_branding_assets,
};
pub use parameter::{
    EnumChoice, PageDescriptor, ParameterDescriptor, ParameterFlags, ParameterKind,
    ParameterSchema, PluginSemanticControl, SuggestedControl,
};
pub use preset::{BankDescriptor, PresetCatalog, PresetDescriptor, ProgramCatalog};
pub use program::{
    MAX_PROGRAM_ARTIFACT_BYTES, MAX_PROGRAM_ARTIFACTS, PROGRAM_EDIT_SCHEMA_VERSION,
    PROGRAM_EDITOR_SCHEMA_VERSION, PROGRAM_SCHEMA_VERSION, PreparedProgram, ProgramArtifact,
    ProgramDocument, ProgramEditRequest, ProgramEditorChoice, ProgramEditorField,
    ProgramEditorFieldKind, ProgramEditorPage, ProgramEditorValue, ProgramEditorView, ProgramError,
    ProgramFieldEditRequest, validate_plugin_identifier, validate_program_identifier,
};
pub use rackforge_control_profile::{
    CONTROL_PROFILE_SCHEMA_VERSION, SemanticControlId, roles as semantic_roles,
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

pub const MIN_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const MANIFEST_SCHEMA_VERSION: u32 = 3;
pub const MIN_PARAMETER_SCHEMA_VERSION: u32 = 1;
/// Schema v2 adds host-owned semantic control declarations. Schema v3 adds an
/// optional plugin-wide display precision without changing parameter steps or
/// runtime resolution. Older schemas remain readable so existing `.rfplugin`
/// packages do not need repackaging.
pub const PARAMETER_SCHEMA_VERSION: u32 = 3;
pub const PRESET_CATALOG_SCHEMA_VERSION: u32 = 1;
pub const RUNTIME_DESCRIPTOR_SCHEMA_VERSION: u32 = 1;
