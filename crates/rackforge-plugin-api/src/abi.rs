//! C ABI v1 used to load native RackForge plugins.
//!
//! No Rust-owned string, vector, trait object, or allocator crosses this
//! boundary. Variable-sized data uses caller-owned byte buffers.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

pub const ABI_VERSION_MAJOR: u16 = 1;
pub const ABI_VERSION_MINOR: u16 = 8;
pub const ABI_VERSION: u32 = pack_version(ABI_VERSION_MAJOR, ABI_VERSION_MINOR);
pub const ENTRY_SYMBOL_V1: &[u8] = b"rackforge_plugin_entry_v1\0";
pub const PROGRAM_EXTENSION_VERSION_MAJOR: u16 = 1;
pub const PROGRAM_EXTENSION_VERSION_MINOR: u16 = 2;
pub const PROGRAM_EXTENSION_VERSION: u32 = pack_version(
    PROGRAM_EXTENSION_VERSION_MAJOR,
    PROGRAM_EXTENSION_VERSION_MINOR,
);
pub const PROGRAM_EXTENSION_ENTRY_SYMBOL_V1: &[u8] = b"rackforge_program_extension_entry_v1\0";
pub const SURFACE_EXTENSION_VERSION_MAJOR: u16 = 1;
pub const SURFACE_EXTENSION_VERSION_MINOR: u16 = 0;
pub const SURFACE_EXTENSION_VERSION: u32 = pack_version(
    SURFACE_EXTENSION_VERSION_MAJOR,
    SURFACE_EXTENSION_VERSION_MINOR,
);
pub const SURFACE_EXTENSION_ENTRY_SYMBOL_V1: &[u8] = b"rackforge_surface_extension_entry_v1\0";

pub const STATUS_OK: i32 = 0;
pub const STATUS_INVALID_ARGUMENT: i32 = -1;
pub const STATUS_INVALID_STATE: i32 = -2;
pub const STATUS_UNKNOWN_PARAMETER: i32 = -3;
pub const STATUS_INTERNAL_ERROR: i32 = -4;

pub const fn pack_version(major: u16, minor: u16) -> u32 {
    ((major as u32) << 16) | minor as u32
}

pub const fn version_major(version: u32) -> u16 {
    (version >> 16) as u16
}

pub const fn version_minor(version: u32) -> u16 {
    version as u16
}

pub const LOG_LEVEL_TRACE: u32 = 0;
pub const LOG_LEVEL_DEBUG: u32 = 1;
pub const LOG_LEVEL_INFO: u32 = 2;
pub const LOG_LEVEL_WARN: u32 = 3;
pub const LOG_LEVEL_ERROR: u32 = 4;

pub type HostLogFnV1 =
    unsafe extern "C" fn(context: *mut c_void, level: u32, text: *const u8, length: usize);
pub type HostGetResourcePathFnV1 = unsafe extern "C" fn(
    context: *mut c_void,
    resource_id: *const u8,
    resource_id_length: usize,
    destination: *mut u8,
    capacity: usize,
) -> usize;
pub type HostGetPluginDataPathFnV1 =
    unsafe extern "C" fn(context: *mut c_void, destination: *mut u8, capacity: usize) -> usize;
pub type HostPublishPresetCatalogFnV1 =
    unsafe extern "C" fn(context: *mut c_void, source: *const u8, length: usize) -> i32;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostApiV1 {
    pub struct_size: u32,
    pub api_version: u32,
    pub context: *mut c_void,
    pub log: Option<HostLogFnV1>,
    pub get_resource_path: Option<HostGetResourcePathFnV1>,
    pub get_plugin_data_path: Option<HostGetPluginDataPathFnV1>,
    pub publish_preset_catalog: Option<HostPublishPresetCatalogFnV1>,
}

impl HostApiV1 {
    pub const fn new(
        context: *mut c_void,
        log: Option<HostLogFnV1>,
        get_resource_path: Option<HostGetResourcePathFnV1>,
        get_plugin_data_path: Option<HostGetPluginDataPathFnV1>,
        publish_preset_catalog: Option<HostPublishPresetCatalogFnV1>,
    ) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            api_version: ABI_VERSION,
            context,
            log,
            get_resource_path,
            get_plugin_data_path,
            publish_preset_catalog,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MidiEventV1 {
    pub frame: u32,
    pub length: u8,
    pub data: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ParameterEventV1 {
    pub frame: u32,
    pub parameter_index: u32,
    pub value: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProcessBlockV1 {
    pub struct_size: u32,
    pub frames: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    pub input_interleaved: *const f32,
    pub output_interleaved: *mut f32,
    pub midi_events: *const MidiEventV1,
    pub midi_event_count: u32,
    pub parameter_events: *const ParameterEventV1,
    pub parameter_event_count: u32,
}

impl ProcessBlockV1 {
    pub const fn empty() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            frames: 0,
            input_channels: 0,
            output_channels: 0,
            input_interleaved: ptr::null(),
            output_interleaved: ptr::null_mut(),
            midi_events: ptr::null(),
            midi_event_count: 0,
            parameter_events: ptr::null(),
            parameter_event_count: 0,
        }
    }
}

pub type WriteBytesFnV1 = unsafe extern "C" fn(destination: *mut u8, capacity: usize) -> usize;
pub type CreateFnV1 = unsafe extern "C" fn(host: *const HostApiV1) -> *mut c_void;
pub type DestroyFnV1 = unsafe extern "C" fn(instance: *mut c_void);
pub type ActivateFnV1 = unsafe extern "C" fn(
    instance: *mut c_void,
    sample_rate: f64,
    maximum_frames: u32,
    input_channels: u32,
    output_channels: u32,
) -> i32;
pub type InstanceFnV1 = unsafe extern "C" fn(instance: *mut c_void) -> i32;
pub type SetParameterFnV1 =
    unsafe extern "C" fn(instance: *mut c_void, parameter_index: u32, value: f64) -> i32;
pub type GetParameterFnV1 =
    unsafe extern "C" fn(instance: *mut c_void, parameter_index: u32, value: *mut f64) -> i32;
pub type SaveStateFnV1 =
    unsafe extern "C" fn(instance: *mut c_void, destination: *mut u8, capacity: usize) -> usize;
pub type LoadStateFnV1 =
    unsafe extern "C" fn(instance: *mut c_void, source: *const u8, length: usize) -> i32;
pub type LoadPresetFnV1 =
    unsafe extern "C" fn(instance: *mut c_void, preset_id: *const u8, length: usize) -> i32;
pub type ProcessFnV1 =
    unsafe extern "C" fn(instance: *mut c_void, block: *const ProcessBlockV1) -> i32;

#[repr(C)]
pub struct PluginApiV1 {
    pub struct_size: u32,
    pub api_version: u32,
    pub runtime_descriptor_json: WriteBytesFnV1,
    pub parameter_schema_json: WriteBytesFnV1,
    pub preset_catalog_json: WriteBytesFnV1,
    pub create: CreateFnV1,
    pub destroy: DestroyFnV1,
    pub activate: ActivateFnV1,
    pub deactivate: InstanceFnV1,
    pub reset: InstanceFnV1,
    pub set_parameter: SetParameterFnV1,
    pub get_parameter: GetParameterFnV1,
    pub save_state: SaveStateFnV1,
    pub load_state: LoadStateFnV1,
    pub load_preset: LoadPresetFnV1,
    pub process: ProcessFnV1,
}

pub type PluginEntryFnV1 = unsafe extern "C" fn() -> *const PluginApiV1;

pub type ProgramExchangeJsonFnV1 = unsafe extern "C" fn(
    instance: *mut c_void,
    source: *const u8,
    source_length: usize,
    destination: *mut u8,
    capacity: usize,
) -> usize;
pub type ProgramInstallFnV1 =
    unsafe extern "C" fn(instance: *mut c_void, source: *const u8, source_length: usize) -> i32;

#[repr(C)]
pub struct ProgramExtensionApiV1 {
    pub struct_size: u32,
    pub api_version: u32,
    pub begin_edit: ProgramExchangeJsonFnV1,
    pub prepare_save: ProgramExchangeJsonFnV1,
    pub install: ProgramInstallFnV1,
    pub preview: Option<ProgramInstallFnV1>,
    pub editor_view: Option<ProgramExchangeJsonFnV1>,
    pub apply_edit: Option<ProgramExchangeJsonFnV1>,
}

pub const PROGRAM_EXTENSION_V1_0_SIZE: u32 =
    std::mem::offset_of!(ProgramExtensionApiV1, preview) as u32;
pub const PROGRAM_EXTENSION_V1_1_SIZE: u32 =
    std::mem::offset_of!(ProgramExtensionApiV1, editor_view) as u32;
pub const PROGRAM_EXTENSION_V1_2_SIZE: u32 = size_of::<ProgramExtensionApiV1>() as u32;

pub type ProgramExtensionEntryFnV1 = unsafe extern "C" fn() -> *const ProgramExtensionApiV1;

#[repr(C)]
pub struct SurfaceExtensionApiV1 {
    pub struct_size: u32,
    pub api_version: u32,
    pub activate: ProgramExchangeJsonFnV1,
}

pub type SurfaceExtensionEntryFnV1 = unsafe extern "C" fn() -> *const SurfaceExtensionApiV1;

/// Copies plugin-owned bytes into a host-owned buffer and returns the required
/// size. Passing a null destination or zero capacity performs a size query.
///
/// # Safety
///
/// If `destination` is non-null, it must be valid for writes of `capacity`
/// bytes. The source slice must remain valid for the duration of the call.
pub unsafe fn copy_to_host_buffer(source: &[u8], destination: *mut u8, capacity: usize) -> usize {
    if !destination.is_null() && capacity > 0 {
        let count = source.len().min(capacity);
        // SAFETY: guaranteed by the caller; the regions cannot overlap because
        // the destination is owned by the host and source by the plugin.
        unsafe {
            ptr::copy_nonoverlapping(source.as_ptr(), destination, count);
        }
    }
    source.len()
}

pub fn is_compatible(plugin_version: u32) -> bool {
    version_major(plugin_version) == ABI_VERSION_MAJOR && plugin_version <= ABI_VERSION
}

pub fn is_program_extension_compatible(version: u32) -> bool {
    version_major(version) == PROGRAM_EXTENSION_VERSION_MAJOR
        && version <= PROGRAM_EXTENSION_VERSION
}

pub fn is_surface_extension_compatible(version: u32) -> bool {
    version_major(version) == SURFACE_EXTENSION_VERSION_MAJOR
        && version <= SURFACE_EXTENSION_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_checks_api_versions() {
        assert_eq!(version_major(ABI_VERSION), 1);
        assert_eq!(version_minor(ABI_VERSION), 8);
        assert!(is_compatible(pack_version(1, 0)));
        assert!(is_compatible(pack_version(1, 1)));
        assert!(is_compatible(pack_version(1, 2)));
        assert!(is_compatible(pack_version(1, 3)));
        assert!(is_compatible(pack_version(1, 4)));
        assert!(is_compatible(pack_version(1, 5)));
        assert!(is_compatible(pack_version(1, 6)));
        assert!(!is_compatible(pack_version(2, 0)));
        assert!(is_compatible(pack_version(1, 7)));
        assert!(is_compatible(pack_version(1, 8)));
        assert!(!is_compatible(pack_version(1, 9)));
        assert!(is_program_extension_compatible(pack_version(1, 0)));
        assert!(is_program_extension_compatible(pack_version(1, 1)));
        assert!(is_program_extension_compatible(pack_version(1, 2)));
        assert!(!is_program_extension_compatible(pack_version(1, 3)));
        assert!(!is_program_extension_compatible(pack_version(2, 0)));
        assert!(is_surface_extension_compatible(pack_version(1, 0)));
        assert!(!is_surface_extension_compatible(pack_version(1, 1)));
        assert!(!is_surface_extension_compatible(pack_version(2, 0)));
    }

    #[test]
    fn reports_required_buffer_size() {
        let source = b"rackforge";
        let mut short = [0_u8; 3];
        // SAFETY: `short` is writable for exactly its capacity.
        let required = unsafe { copy_to_host_buffer(source, short.as_mut_ptr(), short.len()) };
        assert_eq!(required, source.len());
        assert_eq!(&short, b"rac");
    }
}
