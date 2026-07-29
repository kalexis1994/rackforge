use artupy_plugin_api::abi::{
    HostApiV1, LOG_LEVEL_ERROR, LOG_LEVEL_INFO, MidiEventV1, ParameterEventV1, PluginApiV1,
    ProcessBlockV1, STATUS_INVALID_ARGUMENT, STATUS_INVALID_STATE, STATUS_OK,
    STATUS_UNKNOWN_PARAMETER, copy_to_host_buffer, pack_version, version_major, version_minor,
};
use hound::{SampleFormat, WavReader};
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex, OnceLock, Weak};

const FIRST_NOTE: u8 = 36;
const LAST_NOTE: u8 = 96;
const MAX_VOICES: usize = 16;
const RESOURCE_ID: &[u8] = b"rendered-bank";
const MASTER_GAIN_PARAMETER: u32 = 0;
const ATTACK_PARAMETER: u32 = 1;
const RELEASE_PARAMETER: u32 = 2;

const RUNTIME_DESCRIPTOR: &[u8] = br#"{
  "schema_version": 1,
  "id": "org.artupy.roland-scva",
  "version": "0.1.0",
  "state_version": 1
}"#;

const PARAMETER_SCHEMA: &[u8] = br#"{
  "schema_version": 1,
  "pages": [
    { "id": "sound", "name": "Sound", "order": 0 },
    { "id": "envelope", "name": "Envelope", "order": 1 }
  ],
  "parameters": [
    {
      "index": 0,
      "id": "master-gain",
      "name": "Volume",
      "page": "sound",
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
      "id": "envelope.attack",
      "name": "Attack",
      "page": "envelope",
      "order": 0,
      "kind": {
        "type": "float",
        "minimum": 0.0,
        "maximum": 5.0,
        "default": 0.0,
        "step": 0.01,
        "unit": "s"
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
      "index": 2,
      "id": "envelope.release",
      "name": "Release",
      "page": "envelope",
      "order": 1,
      "kind": {
        "type": "float",
        "minimum": 0.005,
        "maximum": 10.0,
        "default": 0.05,
        "step": 0.005,
        "unit": "s"
      },
      "flags": {
        "automatable": true,
        "modulatable": true,
        "read_only": false,
        "advanced": false
      },
      "suggested_control": "knob"
    }
  ]
}"#;

const PRESET_CATALOG: &[u8] = br#"{
  "schema_version": 1,
  "banks": [
    { "id": "scva", "name": "Sound Canvas VA", "order": 0 }
  ],
  "presets": [
    {
      "id": "scva.piano-1",
      "name": "Piano 1",
      "bank": "scva",
      "category": "Piano",
      "order": 0,
      "tags": ["acoustic", "gm"]
    }
  ]
}"#;

struct Sample {
    sample_rate: f64,
    frames: Arc<[f32]>,
}

struct SampleBank {
    notes: BTreeMap<u8, Arc<Sample>>,
}

struct Voice {
    note: u8,
    sample: Arc<Sample>,
    position: f64,
    step: f64,
    velocity_gain: f32,
    age_frames: u64,
    release_gain: f32,
    releasing: bool,
    deferred_release: bool,
    serial: u64,
}

struct RolandScva {
    bank: Arc<SampleBank>,
    voices: Vec<Voice>,
    sample_rate: f64,
    maximum_frames: u32,
    output_channels: u32,
    active: bool,
    sustain: bool,
    serial: u64,
    master_gain: f32,
    attack_seconds: f32,
    release_seconds: f32,
}

impl RolandScva {
    fn new(bank: Arc<SampleBank>) -> Self {
        Self {
            bank,
            voices: Vec::with_capacity(MAX_VOICES),
            sample_rate: 48_000.0,
            maximum_frames: 0,
            output_channels: 0,
            active: false,
            sustain: false,
            serial: 0,
            master_gain: 1.0,
            attack_seconds: 0.0,
            release_seconds: 0.05,
        }
    }

    fn set_parameter(&mut self, index: u32, value: f64) -> i32 {
        if !value.is_finite() {
            return STATUS_INVALID_ARGUMENT;
        }
        match index {
            MASTER_GAIN_PARAMETER if (0.0..=2.0).contains(&value) => {
                self.master_gain = value as f32;
            }
            ATTACK_PARAMETER if (0.0..=5.0).contains(&value) => {
                self.attack_seconds = value as f32;
            }
            RELEASE_PARAMETER if (0.005..=10.0).contains(&value) => {
                self.release_seconds = value as f32;
            }
            MASTER_GAIN_PARAMETER | ATTACK_PARAMETER | RELEASE_PARAMETER => {
                return STATUS_INVALID_ARGUMENT;
            }
            _ => return STATUS_UNKNOWN_PARAMETER,
        }
        STATUS_OK
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        let Some(sample) = self.bank.notes.get(&note).cloned() else {
            return;
        };
        if self.voices.len() == MAX_VOICES {
            let victim = self
                .voices
                .iter()
                .enumerate()
                .min_by_key(|(_, voice)| (!voice.releasing, voice.serial))
                .map(|(index, _)| index)
                .unwrap_or(0);
            self.voices.swap_remove(victim);
        }
        self.serial = self.serial.wrapping_add(1);
        self.voices.push(Voice {
            note,
            step: sample.sample_rate / self.sample_rate,
            sample,
            position: 0.0,
            velocity_gain: (f32::from(velocity) / 127.0).powf(0.7),
            age_frames: 0,
            release_gain: 1.0,
            releasing: false,
            deferred_release: false,
            serial: self.serial,
        });
    }

    fn note_off(&mut self, note: u8) {
        if let Some(voice) = self
            .voices
            .iter_mut()
            .filter(|voice| voice.note == note && !voice.releasing && !voice.deferred_release)
            .min_by_key(|voice| voice.serial)
        {
            if self.sustain {
                voice.deferred_release = true;
            } else {
                voice.releasing = true;
            }
        }
    }

    fn set_sustain(&mut self, enabled: bool) {
        self.sustain = enabled;
        if !enabled {
            for voice in &mut self.voices {
                if voice.deferred_release {
                    voice.deferred_release = false;
                    voice.releasing = true;
                }
            }
        }
    }

    fn handle_midi(&mut self, event: MidiEventV1) {
        if event.length == 0 {
            return;
        }
        let status = event.data[0] & 0xF0;
        match status {
            0x80 if event.length >= 3 => self.note_off(event.data[1] & 0x7F),
            0x90 if event.length >= 3 && event.data[2] != 0 => {
                self.note_on(event.data[1] & 0x7F, event.data[2] & 0x7F);
            }
            0x90 if event.length >= 3 => self.note_off(event.data[1] & 0x7F),
            0xB0 if event.length >= 3 && event.data[1] == 64 => {
                self.set_sustain(event.data[2] >= 64);
            }
            0xB0 if event.length >= 3 && matches!(event.data[1], 120 | 123) => {
                self.voices.clear();
                self.sustain = false;
            }
            _ => {}
        }
    }

    fn render_frame(&mut self) -> f32 {
        let attack_frames = f64::from(self.attack_seconds) * self.sample_rate;
        let release_frames = (f64::from(self.release_seconds) * self.sample_rate).max(1.0) as f32;
        let mut mixed = 0.0_f32;
        let mut index = 0;
        while index < self.voices.len() {
            let voice = &mut self.voices[index];
            let source_index = voice.position.floor() as usize;
            if source_index >= voice.sample.frames.len() {
                self.voices.swap_remove(index);
                continue;
            }
            let first = voice.sample.frames[source_index];
            let second = *voice.sample.frames.get(source_index + 1).unwrap_or(&first);
            let fraction = (voice.position - source_index as f64) as f32;
            let sample = first + (second - first) * fraction;
            let attack_gain = if attack_frames <= 1.0 {
                1.0
            } else {
                ((voice.age_frames as f64 + 1.0) / attack_frames).min(1.0) as f32
            };
            mixed +=
                sample * voice.velocity_gain * attack_gain * voice.release_gain * self.master_gain;
            voice.position += voice.step;
            voice.age_frames = voice.age_frames.saturating_add(1);
            if voice.releasing {
                voice.release_gain -= 1.0 / release_frames;
                if voice.release_gain <= 0.0 {
                    self.voices.swap_remove(index);
                    continue;
                }
            }
            index += 1;
        }
        mixed
    }

    fn reset(&mut self) {
        self.voices.clear();
        self.sustain = false;
    }
}

static BANK_CACHE: OnceLock<Mutex<BTreeMap<PathBuf, Weak<SampleBank>>>> = OnceLock::new();

fn cached_bank(directory: &Path) -> Result<Arc<SampleBank>, String> {
    let cache = BANK_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(bank) = cache
        .lock()
        .map_err(|_| "SCVA bank cache is poisoned".to_owned())?
        .get(directory)
        .and_then(Weak::upgrade)
    {
        return Ok(bank);
    }
    let bank = Arc::new(load_bank(directory)?);
    cache
        .lock()
        .map_err(|_| "SCVA bank cache is poisoned".to_owned())?
        .insert(directory.to_path_buf(), Arc::downgrade(&bank));
    Ok(bank)
}

fn load_bank(directory: &Path) -> Result<SampleBank, String> {
    let mut notes = BTreeMap::new();
    for note in FIRST_NOTE..=LAST_NOTE {
        let path = directory.join(format!("note-{note:03}.wav"));
        let mut reader = WavReader::open(&path)
            .map_err(|error| format!("opening {}: {error}", path.display()))?;
        let specification = reader.spec();
        let channels = usize::from(specification.channels);
        if channels == 0 || specification.sample_rate == 0 {
            return Err(format!("{} has invalid WAV metadata", path.display()));
        }
        let interleaved: Vec<f32> =
            match (specification.sample_format, specification.bits_per_sample) {
                (SampleFormat::Float, 32) => reader
                    .samples::<f32>()
                    .collect::<Result<_, _>>()
                    .map_err(|error| format!("reading {}: {error}", path.display()))?,
                (SampleFormat::Int, 16) => reader
                    .samples::<i16>()
                    .map(|sample| sample.map(|value| f32::from(value) / f32::from(i16::MAX)))
                    .collect::<Result<_, _>>()
                    .map_err(|error| format!("reading {}: {error}", path.display()))?,
                _ => {
                    return Err(format!("{} must be float32 or PCM16 WAV", path.display()));
                }
            };
        let mono: Vec<f32> = interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
            .collect();
        if mono.is_empty() || mono.iter().any(|sample| !sample.is_finite()) {
            return Err(format!("{} contains invalid audio", path.display()));
        }
        notes.insert(
            note,
            Arc::new(Sample {
                sample_rate: f64::from(specification.sample_rate),
                frames: Arc::from(mono),
            }),
        );
    }
    Ok(SampleBank { notes })
}

fn log_host(host: &HostApiV1, level: u32, message: &str) {
    if let Some(log) = host.log {
        // SAFETY: message remains readable for the callback duration.
        unsafe {
            log(host.context, level, message.as_ptr(), message.len());
        }
    }
}

fn resource_path(host: &HostApiV1) -> Result<PathBuf, String> {
    if version_major(host.api_version) != 1
        || version_minor(host.api_version) < 1
        || host.struct_size < size_of::<HostApiV1>() as u32
    {
        return Err("host does not provide ArtuPy resource API 1.1".into());
    }
    let callback = host
        .get_resource_path
        .ok_or_else(|| "host resource callback is unavailable".to_owned())?;
    // SAFETY: RESOURCE_ID is valid for the call and a null destination queries
    // the required path size.
    let required = unsafe {
        callback(
            host.context,
            RESOURCE_ID.as_ptr(),
            RESOURCE_ID.len(),
            ptr::null_mut(),
            0,
        )
    };
    if required == 0 || required > 32 * 1024 {
        return Err("rendered-bank resource is missing or invalid".into());
    }
    let mut bytes = vec![0_u8; required];
    let reported = unsafe {
        callback(
            host.context,
            RESOURCE_ID.as_ptr(),
            RESOURCE_ID.len(),
            bytes.as_mut_ptr(),
            bytes.len(),
        )
    };
    if reported != required {
        return Err("rendered-bank path changed while being read".into());
    }
    let text =
        String::from_utf8(bytes).map_err(|_| "rendered-bank path is not UTF-8".to_owned())?;
    Ok(PathBuf::from(text))
}

unsafe extern "C" fn write_runtime_descriptor(destination: *mut u8, capacity: usize) -> usize {
    unsafe { copy_to_host_buffer(RUNTIME_DESCRIPTOR, destination, capacity) }
}

unsafe extern "C" fn write_parameter_schema(destination: *mut u8, capacity: usize) -> usize {
    unsafe { copy_to_host_buffer(PARAMETER_SCHEMA, destination, capacity) }
}

unsafe extern "C" fn write_preset_catalog(destination: *mut u8, capacity: usize) -> usize {
    unsafe { copy_to_host_buffer(PRESET_CATALOG, destination, capacity) }
}

unsafe extern "C" fn create(host: *const HostApiV1) -> *mut c_void {
    let Some(host) = (unsafe { host.as_ref() }) else {
        return ptr::null_mut();
    };
    let result = resource_path(host).and_then(|path| cached_bank(&path));
    match result {
        Ok(bank) => {
            log_host(host, LOG_LEVEL_INFO, "Roland SCVA rendered bank loaded");
            Box::into_raw(Box::new(RolandScva::new(bank))).cast()
        }
        Err(error) => {
            log_host(host, LOG_LEVEL_ERROR, &error);
            ptr::null_mut()
        }
    }
}

unsafe extern "C" fn destroy(instance: *mut c_void) {
    if !instance.is_null() {
        unsafe {
            drop(Box::from_raw(instance.cast::<RolandScva>()));
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
    let Some(plugin) = (unsafe { instance.cast::<RolandScva>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if !sample_rate.is_finite()
        || sample_rate <= 0.0
        || maximum_frames == 0
        || input_channels != 0
        || output_channels == 0
        || output_channels > 8
    {
        return STATUS_INVALID_ARGUMENT;
    }
    plugin.sample_rate = sample_rate;
    plugin.maximum_frames = maximum_frames;
    plugin.output_channels = output_channels;
    plugin.active = true;
    plugin.reset();
    STATUS_OK
}

unsafe extern "C" fn deactivate(instance: *mut c_void) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RolandScva>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    plugin.reset();
    plugin.active = false;
    STATUS_OK
}

unsafe extern "C" fn reset(instance: *mut c_void) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RolandScva>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    plugin.reset();
    STATUS_OK
}

unsafe extern "C" fn set_parameter(instance: *mut c_void, parameter_index: u32, value: f64) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RolandScva>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    plugin.set_parameter(parameter_index, value)
}

unsafe extern "C" fn get_parameter(
    instance: *mut c_void,
    parameter_index: u32,
    value: *mut f64,
) -> i32 {
    let (Some(plugin), Some(output)) =
        (unsafe { instance.cast::<RolandScva>().as_ref() }, unsafe {
            value.as_mut()
        })
    else {
        return STATUS_INVALID_ARGUMENT;
    };
    *output = match parameter_index {
        MASTER_GAIN_PARAMETER => f64::from(plugin.master_gain),
        ATTACK_PARAMETER => f64::from(plugin.attack_seconds),
        RELEASE_PARAMETER => f64::from(plugin.release_seconds),
        _ => return STATUS_UNKNOWN_PARAMETER,
    };
    STATUS_OK
}

fn state_bytes(plugin: &RolandScva) -> [u8; 16] {
    let mut state = [0_u8; 16];
    state[..4].copy_from_slice(b"RSV1");
    state[4..8].copy_from_slice(&plugin.master_gain.to_le_bytes());
    state[8..12].copy_from_slice(&plugin.attack_seconds.to_le_bytes());
    state[12..16].copy_from_slice(&plugin.release_seconds.to_le_bytes());
    state
}

unsafe extern "C" fn save_state(
    instance: *mut c_void,
    destination: *mut u8,
    capacity: usize,
) -> usize {
    let Some(plugin) = (unsafe { instance.cast::<RolandScva>().as_ref() }) else {
        return 0;
    };
    let bytes = state_bytes(plugin);
    unsafe { copy_to_host_buffer(&bytes, destination, capacity) }
}

unsafe extern "C" fn load_state(instance: *mut c_void, source: *const u8, length: usize) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RolandScva>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if source.is_null() || length != 16 {
        return STATUS_INVALID_ARGUMENT;
    }
    let bytes = unsafe { slice::from_raw_parts(source, length) };
    if &bytes[..4] != b"RSV1" {
        return STATUS_INVALID_ARGUMENT;
    }
    let values = [
        f32::from_le_bytes(bytes[4..8].try_into().expect("fixed slice")),
        f32::from_le_bytes(bytes[8..12].try_into().expect("fixed slice")),
        f32::from_le_bytes(bytes[12..16].try_into().expect("fixed slice")),
    ];
    for (index, value) in values.into_iter().enumerate() {
        let status = plugin.set_parameter(index as u32, f64::from(value));
        if status != STATUS_OK {
            return status;
        }
    }
    STATUS_OK
}

unsafe extern "C" fn load_preset(
    instance: *mut c_void,
    preset_id: *const u8,
    length: usize,
) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RolandScva>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if preset_id.is_null() || length == 0 {
        return STATUS_INVALID_ARGUMENT;
    }
    let id = unsafe { slice::from_raw_parts(preset_id, length) };
    if id != b"scva.piano-1" {
        return STATUS_INVALID_ARGUMENT;
    }
    plugin.master_gain = 1.0;
    plugin.attack_seconds = 0.0;
    plugin.release_seconds = 0.05;
    plugin.reset();
    STATUS_OK
}

unsafe extern "C" fn process(instance: *mut c_void, block: *const ProcessBlockV1) -> i32 {
    let (Some(plugin), Some(block)) = (unsafe { instance.cast::<RolandScva>().as_mut() }, unsafe {
        block.as_ref()
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if !plugin.active
        || block.struct_size < size_of::<ProcessBlockV1>() as u32
        || block.frames == 0
        || block.frames > plugin.maximum_frames
        || block.input_channels != 0
        || block.output_channels != plugin.output_channels
        || block.output_interleaved.is_null()
    {
        return STATUS_INVALID_STATE;
    }
    let output_length = block.frames as usize * block.output_channels as usize;
    let output = unsafe { slice::from_raw_parts_mut(block.output_interleaved, output_length) };
    output.fill(0.0);
    let midi = unsafe {
        event_slice(
            block.midi_events,
            block.midi_event_count,
            block.frames,
            |event: &MidiEventV1| event.frame,
        )
    };
    let parameters = unsafe {
        event_slice(
            block.parameter_events,
            block.parameter_event_count,
            block.frames,
            |event: &ParameterEventV1| event.frame,
        )
    };
    let (Ok(midi), Ok(parameters)) = (midi, parameters) else {
        return STATUS_INVALID_ARGUMENT;
    };

    let mut midi_index = 0;
    let mut parameter_index = 0;
    for frame in 0..block.frames {
        while parameter_index < parameters.len() && parameters[parameter_index].frame == frame {
            let event = parameters[parameter_index];
            let status = plugin.set_parameter(event.parameter_index, event.value);
            if status != STATUS_OK {
                return status;
            }
            parameter_index += 1;
        }
        while midi_index < midi.len() && midi[midi_index].frame == frame {
            plugin.handle_midi(midi[midi_index]);
            midi_index += 1;
        }
        let sample = plugin.render_frame();
        let start = frame as usize * block.output_channels as usize;
        for channel in 0..block.output_channels as usize {
            output[start + channel] = sample;
        }
    }
    STATUS_OK
}

unsafe fn event_slice<'a, T, F>(
    pointer: *const T,
    count: u32,
    frames: u32,
    frame_of: F,
) -> Result<&'a [T], ()>
where
    F: Fn(&T) -> u32,
{
    if count == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() {
        return Err(());
    }
    let events = unsafe { slice::from_raw_parts(pointer, count as usize) };
    if events.iter().any(|event| frame_of(event) >= frames)
        || events
            .windows(2)
            .any(|pair| frame_of(&pair[0]) > frame_of(&pair[1]))
    {
        return Err(());
    }
    Ok(events)
}

static PLUGIN_API: PluginApiV1 = PluginApiV1 {
    struct_size: size_of::<PluginApiV1>() as u32,
    api_version: pack_version(1, 1),
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

    fn synthetic_plugin() -> RolandScva {
        let sample = Arc::new(Sample {
            sample_rate: 48_000.0,
            frames: Arc::from(vec![0.5_f32; 32]),
        });
        RolandScva::new(Arc::new(SampleBank {
            notes: BTreeMap::from([(60, sample)]),
        }))
    }

    #[test]
    fn exports_valid_metadata() {
        let descriptor: RuntimeDescriptor = serde_json::from_slice(RUNTIME_DESCRIPTOR).unwrap();
        assert_eq!(descriptor.id, "org.artupy.roland-scva");
        let parameters: ParameterSchema = serde_json::from_slice(PARAMETER_SCHEMA).unwrap();
        assert_eq!(parameters.validate(), Ok(()));
        let presets: PresetCatalog = serde_json::from_slice(PRESET_CATALOG).unwrap();
        assert_eq!(presets.validate(), Ok(()));
    }

    #[test]
    fn renders_a_synthetic_voice_without_allocating_more_capacity() {
        let mut plugin = synthetic_plugin();
        plugin.active = true;
        plugin.maximum_frames = 32;
        plugin.output_channels = 2;
        let capacity = plugin.voices.capacity();
        plugin.note_on(60, 127);
        assert_eq!(plugin.render_frame(), 0.5);
        assert_eq!(plugin.voices.capacity(), capacity);
    }

    #[test]
    fn sustain_defers_release_until_the_pedal_lifts() {
        let mut plugin = synthetic_plugin();
        plugin.note_on(60, 100);
        plugin.set_sustain(true);
        plugin.note_off(60);
        assert!(plugin.voices[0].deferred_release);
        assert!(!plugin.voices[0].releasing);
        plugin.set_sustain(false);
        assert!(!plugin.voices[0].deferred_release);
        assert!(plugin.voices[0].releasing);
    }
}
