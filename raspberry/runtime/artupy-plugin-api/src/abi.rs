//! C ABI v1 used to load native ArtuPy plugins.
//!
//! No Rust-owned string, vector, trait object, or allocator crosses this
//! boundary. Variable-sized data uses caller-owned byte buffers.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;

pub const ABI_VERSION_MAJOR: u16 = 1;
pub const ABI_VERSION_MINOR: u16 = 0;
pub const ABI_VERSION: u32 = pack_version(ABI_VERSION_MAJOR, ABI_VERSION_MINOR);
pub const ENTRY_SYMBOL_V1: &[u8] = b"artupy_plugin_entry_v1\0";

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

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostApiV1 {
    pub struct_size: u32,
    pub api_version: u32,
    pub context: *mut c_void,
    pub log: Option<HostLogFnV1>,
}

impl HostApiV1 {
    pub const fn new(context: *mut c_void, log: Option<HostLogFnV1>) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            api_version: ABI_VERSION,
            context,
            log,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_and_checks_api_versions() {
        assert_eq!(version_major(ABI_VERSION), 1);
        assert_eq!(version_minor(ABI_VERSION), 0);
        assert!(is_compatible(pack_version(1, 0)));
        assert!(!is_compatible(pack_version(2, 0)));
        assert!(!is_compatible(pack_version(1, 1)));
    }

    #[test]
    fn reports_required_buffer_size() {
        let source = b"artupy";
        let mut short = [0_u8; 3];
        // SAFETY: `short` is writable for exactly its capacity.
        let required = unsafe { copy_to_host_buffer(source, short.as_mut_ptr(), short.len()) };
        assert_eq!(required, source.len());
        assert_eq!(&short, b"art");
    }
}
