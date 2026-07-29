use artupy_plugin_api::abi::{
    HostApiV1, PluginApiV1, ProcessBlockV1, STATUS_INVALID_ARGUMENT, STATUS_INVALID_STATE,
    STATUS_OK, STATUS_UNKNOWN_PARAMETER, copy_to_host_buffer, pack_version,
};
use std::ffi::c_void;
use std::mem::size_of;
use std::ptr;
use std::slice;

const GAIN_PARAMETER: u32 = 0;
const BYPASS_PARAMETER: u32 = 1;

const RUNTIME_DESCRIPTOR: &[u8] = br#"{
  "schema_version": 1,
  "id": "org.artupy.gain",
  "version": "0.1.0",
  "state_version": 1
}"#;

const PARAMETER_SCHEMA: &[u8] = br#"{
  "schema_version": 1,
  "pages": [
    { "id": "level", "name": "Level", "order": 0 }
  ],
  "parameters": [
    {
      "index": 0,
      "id": "gain",
      "name": "Gain",
      "page": "level",
      "order": 0,
      "kind": {
        "type": "float",
        "minimum": 0.0,
        "maximum": 2.0,
        "default": 1.0,
        "step": 0.01,
        "unit": "x"
      },
      "flags": {
        "automatable": true,
        "modulatable": true,
        "read_only": false,
        "advanced": false
      },
      "suggested_control": "knob"
    },
    {
      "index": 1,
      "id": "bypass",
      "name": "Bypass",
      "page": "level",
      "order": 1,
      "kind": {
        "type": "boolean",
        "default": false
      },
      "flags": {
        "automatable": true,
        "modulatable": false,
        "read_only": false,
        "advanced": false
      },
      "suggested_control": "toggle"
    }
  ]
}"#;

const PRESET_CATALOG: &[u8] = br#"{
  "schema_version": 1,
  "banks": [
    { "id": "factory", "name": "Factory", "order": 0 }
  ],
  "presets": [
    {
      "id": "factory.unity",
      "name": "Unity",
      "bank": "factory",
      "category": "Utility",
      "order": 0,
      "tags": ["neutral"]
    },
    {
      "id": "factory.half",
      "name": "Half",
      "bank": "factory",
      "category": "Utility",
      "order": 1,
      "tags": ["attenuation"]
    },
    {
      "id": "factory.boost",
      "name": "Boost",
      "bank": "factory",
      "category": "Utility",
      "order": 2,
      "tags": ["gain"]
    }
  ]
}"#;

struct Gain {
    gain: f32,
    bypass: bool,
    active: bool,
    maximum_frames: u32,
    input_channels: u32,
    output_channels: u32,
}

impl Default for Gain {
    fn default() -> Self {
        Self {
            gain: 1.0,
            bypass: false,
            active: false,
            maximum_frames: 0,
            input_channels: 0,
            output_channels: 0,
        }
    }
}

unsafe extern "C" fn write_runtime_descriptor(destination: *mut u8, capacity: usize) -> usize {
    // SAFETY: the host owns and describes the destination buffer.
    unsafe { copy_to_host_buffer(RUNTIME_DESCRIPTOR, destination, capacity) }
}

unsafe extern "C" fn write_parameter_schema(destination: *mut u8, capacity: usize) -> usize {
    // SAFETY: the host owns and describes the destination buffer.
    unsafe { copy_to_host_buffer(PARAMETER_SCHEMA, destination, capacity) }
}

unsafe extern "C" fn write_preset_catalog(destination: *mut u8, capacity: usize) -> usize {
    // SAFETY: the host owns and describes the destination buffer.
    unsafe { copy_to_host_buffer(PRESET_CATALOG, destination, capacity) }
}

unsafe extern "C" fn create(_host: *const HostApiV1) -> *mut c_void {
    Box::into_raw(Box::new(Gain::default())).cast()
}

unsafe extern "C" fn destroy(instance: *mut c_void) {
    if !instance.is_null() {
        // SAFETY: the pointer was returned by create and is destroyed once.
        unsafe {
            drop(Box::from_raw(instance.cast::<Gain>()));
        }
    }
}

unsafe extern "C" fn activate(
    instance: *mut c_void,
    sample_rate: f64,
    maximum_frames: u32,
    input_channels: u32,
    output_channels: u32,
) -> i32 {
    let Some(gain) = (unsafe { instance.cast::<Gain>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if !sample_rate.is_finite()
        || sample_rate <= 0.0
        || maximum_frames == 0
        || input_channels == 0
        || output_channels == 0
    {
        return STATUS_INVALID_ARGUMENT;
    }
    gain.maximum_frames = maximum_frames;
    gain.input_channels = input_channels;
    gain.output_channels = output_channels;
    gain.active = true;
    STATUS_OK
}

unsafe extern "C" fn deactivate(instance: *mut c_void) -> i32 {
    let Some(gain) = (unsafe { instance.cast::<Gain>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    gain.active = false;
    STATUS_OK
}

unsafe extern "C" fn reset(instance: *mut c_void) -> i32 {
    let Some(_gain) = (unsafe { instance.cast::<Gain>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    STATUS_OK
}

fn set_parameter_value(gain: &mut Gain, parameter_index: u32, value: f64) -> i32 {
    if !value.is_finite() {
        return STATUS_INVALID_ARGUMENT;
    }
    match parameter_index {
        GAIN_PARAMETER if (0.0..=2.0).contains(&value) => {
            gain.gain = value as f32;
            STATUS_OK
        }
        BYPASS_PARAMETER if (0.0..=1.0).contains(&value) => {
            gain.bypass = value >= 0.5;
            STATUS_OK
        }
        GAIN_PARAMETER | BYPASS_PARAMETER => STATUS_INVALID_ARGUMENT,
        _ => STATUS_UNKNOWN_PARAMETER,
    }
}

unsafe extern "C" fn set_parameter(instance: *mut c_void, parameter_index: u32, value: f64) -> i32 {
    let Some(gain) = (unsafe { instance.cast::<Gain>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    set_parameter_value(gain, parameter_index, value)
}

unsafe extern "C" fn get_parameter(
    instance: *mut c_void,
    parameter_index: u32,
    value: *mut f64,
) -> i32 {
    let (Some(gain), Some(output)) = (unsafe { instance.cast::<Gain>().as_ref() }, unsafe {
        value.as_mut()
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    *output = match parameter_index {
        GAIN_PARAMETER => f64::from(gain.gain),
        BYPASS_PARAMETER => f64::from(u8::from(gain.bypass)),
        _ => return STATUS_UNKNOWN_PARAMETER,
    };
    STATUS_OK
}

fn state_bytes(gain: &Gain) -> [u8; 9] {
    let mut state = [0_u8; 9];
    state[..4].copy_from_slice(b"AGV1");
    state[4..8].copy_from_slice(&gain.gain.to_le_bytes());
    state[8] = u8::from(gain.bypass);
    state
}

unsafe extern "C" fn save_state(
    instance: *mut c_void,
    destination: *mut u8,
    capacity: usize,
) -> usize {
    let Some(gain) = (unsafe { instance.cast::<Gain>().as_ref() }) else {
        return 0;
    };
    let bytes = state_bytes(gain);
    // SAFETY: the host owns and describes the destination buffer.
    unsafe { copy_to_host_buffer(&bytes, destination, capacity) }
}

unsafe extern "C" fn load_state(instance: *mut c_void, source: *const u8, length: usize) -> i32 {
    let Some(gain) = (unsafe { instance.cast::<Gain>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if source.is_null() || length != 9 {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: source is non-null and the host promises `length` readable bytes.
    let bytes = unsafe { slice::from_raw_parts(source, length) };
    if &bytes[..4] != b"AGV1" || bytes[8] > 1 {
        return STATUS_INVALID_ARGUMENT;
    }
    let value = f32::from_le_bytes(bytes[4..8].try_into().expect("fixed slice"));
    if !value.is_finite() || !(0.0..=2.0).contains(&value) {
        return STATUS_INVALID_ARGUMENT;
    }
    gain.gain = value;
    gain.bypass = bytes[8] != 0;
    STATUS_OK
}

unsafe extern "C" fn load_preset(
    instance: *mut c_void,
    preset_id: *const u8,
    length: usize,
) -> i32 {
    let Some(gain) = (unsafe { instance.cast::<Gain>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if preset_id.is_null() || length == 0 {
        return STATUS_INVALID_ARGUMENT;
    }
    // SAFETY: the host promises `length` readable bytes for the call.
    let bytes = unsafe { slice::from_raw_parts(preset_id, length) };
    match bytes {
        b"factory.unity" => {
            gain.gain = 1.0;
            gain.bypass = false;
        }
        b"factory.half" => {
            gain.gain = 0.5;
            gain.bypass = false;
        }
        b"factory.boost" => {
            gain.gain = 1.5;
            gain.bypass = false;
        }
        _ => return STATUS_INVALID_ARGUMENT,
    }
    STATUS_OK
}

unsafe extern "C" fn process(instance: *mut c_void, block: *const ProcessBlockV1) -> i32 {
    let (Some(gain), Some(block)) = (unsafe { instance.cast::<Gain>().as_mut() }, unsafe {
        block.as_ref()
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if !gain.active
        || block.struct_size < size_of::<ProcessBlockV1>() as u32
        || block.frames > gain.maximum_frames
        || block.input_channels != gain.input_channels
        || block.output_channels != gain.output_channels
        || block.input_interleaved.is_null()
        || block.output_interleaved.is_null()
    {
        return STATUS_INVALID_STATE;
    }

    let input_length = block.frames as usize * block.input_channels as usize;
    let output_length = block.frames as usize * block.output_channels as usize;
    // SAFETY: the host provides buffers sized according to the block fields.
    let input = unsafe { slice::from_raw_parts(block.input_interleaved, input_length) };
    let output = unsafe { slice::from_raw_parts_mut(block.output_interleaved, output_length) };
    let events = if block.parameter_event_count == 0 {
        &[][..]
    } else {
        if block.parameter_events.is_null() {
            return STATUS_INVALID_ARGUMENT;
        }
        unsafe {
            slice::from_raw_parts(block.parameter_events, block.parameter_event_count as usize)
        }
    };
    if events.windows(2).any(|pair| pair[0].frame > pair[1].frame)
        || events.iter().any(|event| event.frame >= block.frames)
    {
        return STATUS_INVALID_ARGUMENT;
    }

    let mut event_index = 0;
    for frame in 0..block.frames {
        while event_index < events.len() && events[event_index].frame == frame {
            let event = events[event_index];
            let status = set_parameter_value(gain, event.parameter_index, event.value);
            if status != STATUS_OK {
                return status;
            }
            event_index += 1;
        }
        for channel in 0..block.output_channels {
            let input_channel = channel.min(block.input_channels - 1);
            let source =
                input[frame as usize * block.input_channels as usize + input_channel as usize];
            output[frame as usize * block.output_channels as usize + channel as usize] =
                if gain.bypass {
                    source
                } else {
                    source * gain.gain
                };
        }
    }
    STATUS_OK
}

static PLUGIN_API: PluginApiV1 = PluginApiV1 {
    struct_size: size_of::<PluginApiV1>() as u32,
    api_version: pack_version(1, 0),
    runtime_descriptor_json: write_runtime_descriptor,
    parameter_schema_json: write_parameter_schema,
    preset_catalog_json: write_preset_catalog,
    create,
    destroy,
    activate,
    deactivate,
    reset,
    set_parameter,
    get_parameter,
    save_state,
    load_state,
    load_preset,
    process,
};

#[unsafe(no_mangle)]
pub extern "C" fn artupy_plugin_entry_v1() -> *const PluginApiV1 {
    ptr::addr_of!(PLUGIN_API)
}

#[cfg(test)]
mod tests {
    use super::*;
    use artupy_plugin_api::{ParameterSchema, PresetCatalog, RuntimeDescriptor};

    #[test]
    fn exports_valid_declarative_metadata() {
        let descriptor: RuntimeDescriptor = serde_json::from_slice(RUNTIME_DESCRIPTOR).unwrap();
        assert_eq!(descriptor.id, "org.artupy.gain");
        let schema: ParameterSchema = serde_json::from_slice(PARAMETER_SCHEMA).unwrap();
        assert_eq!(schema.validate(), Ok(()));
        let presets: PresetCatalog = serde_json::from_slice(PRESET_CATALOG).unwrap();
        assert_eq!(presets.validate(), Ok(()));
    }

    #[test]
    fn round_trips_state() {
        let mut gain = Gain {
            gain: 0.25,
            bypass: true,
            ..Gain::default()
        };
        let state = state_bytes(&gain);
        gain.gain = 2.0;
        gain.bypass = false;
        let status =
            unsafe { load_state((&mut gain as *mut Gain).cast(), state.as_ptr(), state.len()) };
        assert_eq!(status, STATUS_OK);
        assert_eq!(gain.gain, 0.25);
        assert!(gain.bypass);
    }
}
