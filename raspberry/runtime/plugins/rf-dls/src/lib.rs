mod custom_program;

use custom_program::{CustomProgram, ProgramParameters};
use rackforge_plugin_api::abi::{
    HostApiV1, LOG_LEVEL_ERROR, LOG_LEVEL_INFO, LOG_LEVEL_WARN, MidiEventV1, ParameterEventV1,
    PluginApiV1, ProcessBlockV1, STATUS_INVALID_ARGUMENT, STATUS_INVALID_STATE, STATUS_OK,
    STATUS_UNKNOWN_PARAMETER, copy_to_host_buffer, pack_version, version_major, version_minor,
};
use rackforge_plugin_api::{BankDescriptor, PresetCatalog, PresetDescriptor};
use rf_dls::{DlsBank, EnvelopeSpec, Voice};
use std::collections::BTreeSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::path::PathBuf;
use std::ptr;
use std::slice;

const RESOURCE_ID: &[u8] = b"dls-bank";
const PIANO_PRESET_ID: &str = "gm.piano-1";
const MASTER_GAIN_PARAMETER: u32 = 0;
const DEFAULT_MASTER_GAIN: f32 = 0.65;
const DEFAULT_PITCH_BEND_RANGE_CENTS: f32 = 200.0;
const MAX_VOICES: usize = 32;
const LEGACY_STATE_SIZE: usize = 16;

const RUNTIME_DESCRIPTOR: &[u8] = br#"{
  "schema_version": 1,
  "id": "org.rackforge.rf-dls",
  "version": "0.1.0",
  "state_version": 2
}"#;

const PARAMETER_SCHEMA: &[u8] = br#"{
  "schema_version": 1,
  "pages": [
    { "id": "sound", "name": "Sound", "order": 0, "header": "RF-DLS" }
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
        "maximum": 1.0,
        "default": 0.65,
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
    }
  ]
}"#;

const PRESET_CATALOG: &[u8] = br#"{
  "schema_version": 1,
  "banks": [
    { "id": "gm", "name": "General MIDI", "order": 0 }
  ],
  "presets": [
    {
      "id": "gm.piano-1",
      "name": "Acoustic Grand Piano",
      "bank": "gm",
      "category": "Piano",
      "order": 0,
      "tags": ["acoustic", "gm"]
    }
  ]
}"#;

struct RfDls {
    bank: DlsBank,
    voices: Vec<Voice>,
    held_notes: [bool; 128],
    sample_rate: u32,
    maximum_frames: u32,
    output_channels: u32,
    active: bool,
    sustain: bool,
    pitch_bend_cents: f32,
    pitch_bend_range_cents: f32,
    modulation_wheel: f32,
    master_gain: f32,
    program_gain: f32,
    program_pitch_offset_cents: f32,
    program_modulation_depth: f32,
    envelope_override: Option<EnvelopeSpec>,
    selected_bank: u32,
    selected_program: u32,
    selected_preset_id: String,
    custom_programs: Vec<CustomProgram>,
}

impl RfDls {
    fn new(
        bank: DlsBank,
        selected_bank: u32,
        selected_program: u32,
        custom_programs: Vec<CustomProgram>,
    ) -> Self {
        Self {
            bank,
            voices: Vec::with_capacity(MAX_VOICES),
            held_notes: [false; 128],
            sample_rate: 48_000,
            maximum_frames: 0,
            output_channels: 0,
            active: false,
            sustain: false,
            pitch_bend_cents: 0.0,
            pitch_bend_range_cents: DEFAULT_PITCH_BEND_RANGE_CENTS,
            modulation_wheel: 0.0,
            master_gain: DEFAULT_MASTER_GAIN,
            program_gain: 1.0,
            program_pitch_offset_cents: 0.0,
            program_modulation_depth: 1.0,
            envelope_override: None,
            selected_bank,
            selected_program,
            selected_preset_id: dynamic_preset_id(selected_bank, selected_program),
            custom_programs,
        }
    }

    fn reset(&mut self) {
        self.voices.clear();
        self.held_notes.fill(false);
        self.sustain = false;
        self.pitch_bend_cents = 0.0;
        self.modulation_wheel = 0.0;
    }

    fn select_instrument(&mut self, bank: u32, program: u32) -> bool {
        if self.bank.instrument(bank, program).is_none() {
            return false;
        }
        self.selected_bank = bank;
        self.selected_program = program;
        self.selected_preset_id = dynamic_preset_id(bank, program);
        self.apply_program_parameters(ProgramParameters::default(), None);
        self.reset();
        true
    }

    fn select_custom(&mut self, preset_id: &str) -> bool {
        let Some(program) = self
            .custom_programs
            .iter()
            .find(|program| program.preset_id() == preset_id)
            .cloned()
        else {
            return false;
        };
        let Some(instrument) = self
            .bank
            .instrument(program.source.bank, program.source.program)
        else {
            return false;
        };
        let envelope = program.resolved_envelope(instrument.envelope);
        self.selected_bank = program.source.bank;
        self.selected_program = program.source.program;
        self.selected_preset_id = preset_id.to_owned();
        self.apply_program_parameters(program.parameters, Some(envelope));
        self.reset();
        true
    }

    fn select_preset(&mut self, preset_id: &str) -> bool {
        if preset_id.starts_with("custom.") {
            return self.select_custom(preset_id);
        }
        let selection = if preset_id == PIANO_PRESET_ID {
            Some((0, 0))
        } else {
            parse_dynamic_preset_id(preset_id)
        };
        selection.is_some_and(|(bank, program)| self.select_instrument(bank, program))
    }

    fn apply_program_parameters(
        &mut self,
        parameters: ProgramParameters,
        envelope: Option<EnvelopeSpec>,
    ) {
        self.program_gain = parameters.gain;
        self.program_pitch_offset_cents =
            f32::from(parameters.transpose_semitones) * 100.0 + parameters.fine_tune_cents;
        self.pitch_bend_range_cents = parameters.pitch_bend_range_semitones * 100.0;
        self.program_modulation_depth = parameters.modulation_depth;
        self.envelope_override = envelope;
    }

    fn set_parameter(&mut self, index: u32, value: f64) -> i32 {
        if !value.is_finite() {
            return STATUS_INVALID_ARGUMENT;
        }
        match index {
            MASTER_GAIN_PARAMETER if (0.0..=1.0).contains(&value) => {
                self.master_gain = value as f32;
                STATUS_OK
            }
            MASTER_GAIN_PARAMETER => STATUS_INVALID_ARGUMENT,
            _ => STATUS_UNKNOWN_PARAMETER,
        }
    }

    fn note_on(&mut self, note: u8, velocity: u8) {
        self.held_notes[note as usize] = true;
        self.voices.retain(|voice| voice.note != note);
        let bank = &self.bank;
        let Some(instrument) = bank.instrument(self.selected_bank, self.selected_program) else {
            return;
        };
        for region in instrument.matching_regions(note, velocity) {
            let envelope = self.envelope_override.unwrap_or(instrument.envelope);
            if let Ok(voice) = Voice::new_with_envelope(
                bank,
                instrument,
                region,
                note,
                velocity,
                self.sample_rate,
                envelope,
            ) {
                if self.voices.len() == MAX_VOICES {
                    self.voices.remove(0);
                }
                self.voices.push(voice);
            }
        }
    }

    fn note_off(&mut self, note: u8) {
        self.held_notes[note as usize] = false;
        if !self.sustain {
            self.release_note(note);
        }
    }

    fn release_note(&mut self, note: u8) {
        for voice in &mut self.voices {
            if voice.note == note {
                voice.note_off();
            }
        }
    }

    fn set_sustain(&mut self, enabled: bool) {
        let was_enabled = self.sustain;
        self.sustain = enabled;
        if was_enabled && !enabled {
            for voice in &mut self.voices {
                if !self.held_notes[voice.note as usize] {
                    voice.note_off();
                }
            }
        }
    }

    fn all_notes_off(&mut self) {
        self.held_notes.fill(false);
        if !self.sustain {
            for voice in &mut self.voices {
                voice.note_off();
            }
        }
    }

    fn all_sound_off(&mut self) {
        self.reset();
    }

    fn set_pitch_bend(&mut self, least_significant: u8, most_significant: u8) {
        let raw = i32::from(least_significant & 0x7f) | (i32::from(most_significant & 0x7f) << 7);
        let centered = raw - 8_192;
        let normalized = if centered >= 0 {
            centered as f32 / 8_191.0
        } else {
            centered as f32 / 8_192.0
        };
        self.pitch_bend_cents = normalized * self.pitch_bend_range_cents;
    }

    fn set_modulation_wheel(&mut self, value: u8) {
        self.modulation_wheel = f32::from(value & 0x7f) / 127.0;
    }

    fn reset_controllers(&mut self) {
        self.set_sustain(false);
        self.pitch_bend_cents = 0.0;
        self.modulation_wheel = 0.0;
    }

    fn handle_midi(&mut self, event: MidiEventV1) {
        if event.length == 0 {
            return;
        }
        let status = event.data[0] & 0xf0;
        match status {
            0x80 if event.length >= 3 => self.note_off(event.data[1] & 0x7f),
            0x90 if event.length >= 3 && event.data[2] != 0 => {
                self.note_on(event.data[1] & 0x7f, event.data[2] & 0x7f);
            }
            0x90 if event.length >= 3 => self.note_off(event.data[1] & 0x7f),
            0xe0 if event.length >= 3 => self.set_pitch_bend(event.data[1], event.data[2]),
            0xb0 if event.length >= 3 && event.data[1] == 1 => {
                self.set_modulation_wheel(event.data[2]);
            }
            0xb0 if event.length >= 3 && event.data[1] == 64 => {
                self.set_sustain(event.data[2] >= 64);
            }
            0xb0 if event.length >= 3 && event.data[1] == 120 => self.all_sound_off(),
            0xb0 if event.length >= 3 && event.data[1] == 121 => self.reset_controllers(),
            0xb0 if event.length >= 3 && event.data[1] == 123 => self.all_notes_off(),
            _ => {}
        }
    }

    fn render_frame(&mut self) -> f32 {
        let mut mixed = 0.0_f32;
        let mut index = 0;
        while index < self.voices.len() {
            mixed += self.voices[index].next_sample_modulated(
                self.pitch_bend_cents + self.program_pitch_offset_cents,
                self.modulation_wheel * self.program_modulation_depth,
            );
            if self.voices[index].is_finished() {
                self.voices.swap_remove(index);
            } else {
                index += 1;
            }
        }
        mixed * self.master_gain * self.program_gain
    }
}

fn log_host(host: &HostApiV1, level: u32, message: &str) {
    if let Some(log) = host.log {
        // SAFETY: the message remains readable for the callback duration.
        unsafe {
            log(host.context, level, message.as_ptr(), message.len());
        }
    }
}

fn dynamic_preset_id(bank: u32, program: u32) -> String {
    format!("dls.b{bank:08x}.p{program:08x}")
}

fn parse_dynamic_preset_id(id: &str) -> Option<(u32, u32)> {
    let (bank, program) = id.strip_prefix("dls.b")?.split_once(".p")?;
    if bank.len() != 8 || program.len() != 8 {
        return None;
    }
    Some((
        u32::from_str_radix(bank, 16).ok()?,
        u32::from_str_radix(program, 16).ok()?,
    ))
}

fn dynamic_catalog(bank: &DlsBank, custom_programs: &[CustomProgram]) -> PresetCatalog {
    let mut seen = BTreeSet::new();
    let mut presets = Vec::new();
    let mut instruments = bank.instruments.iter().collect::<Vec<_>>();
    instruments.sort_by_key(|instrument| {
        (
            instrument.is_drum(),
            instrument.bank & 0x7fff_ffff,
            instrument.program,
        )
    });
    for instrument in instruments {
        if !seen.insert((instrument.bank, instrument.program)) {
            continue;
        }
        let is_drum = instrument.is_drum();
        let display_bank = instrument.bank & 0x7fff_ffff;
        let fallback = format!(
            "{} {}",
            if is_drum { "Drum Kit" } else { "Instrument" },
            instrument.program
        );
        let name = if instrument.name.trim().is_empty() {
            fallback
        } else {
            instrument.name.trim().to_owned()
        };
        let description = if is_drum {
            format!("DRUM P{:03}", instrument.program)
        } else {
            format!("B{display_bank:03} P{:03}", instrument.program)
        };
        presets.push(PresetDescriptor {
            id: dynamic_preset_id(instrument.bank, instrument.program),
            name,
            description: Some(description),
            bank: Some("dls".into()),
            category: Some(if is_drum { "Drums" } else { "Instrument" }.into()),
            order: presets.len() as i32,
            tags: vec![if is_drum { "drums" } else { "melodic" }.into()],
        });
    }
    for program in custom_programs {
        presets.push(PresetDescriptor {
            id: program.preset_id(),
            name: program.name.clone(),
            description: Some(format!("CUSTOM {:03}", program.slot)),
            bank: Some("custom".into()),
            category: program.category.clone(),
            order: i32::from(program.slot),
            tags: vec!["custom".into()],
        });
    }
    PresetCatalog {
        schema_version: 1,
        banks: vec![
            BankDescriptor {
                id: "dls".into(),
                name: "DLS".into(),
                order: 0,
            },
            BankDescriptor {
                id: "custom".into(),
                name: "CUSTOM".into(),
                order: 1,
            },
        ],
        presets,
    }
}

fn publish_dynamic_catalog(
    host: &HostApiV1,
    bank: &DlsBank,
    custom_programs: &[CustomProgram],
) -> Result<(), String> {
    if version_major(host.api_version) != 1
        || version_minor(host.api_version) < 3
        || host.struct_size < size_of::<HostApiV1>() as u32
    {
        return Err("host does not provide dynamic preset API 1.3".into());
    }
    let callback = host
        .publish_preset_catalog
        .ok_or_else(|| "host dynamic preset callback is unavailable".to_owned())?;
    let bytes = serde_json::to_vec(&dynamic_catalog(bank, custom_programs))
        .map_err(|error| error.to_string())?;
    // SAFETY: bytes remain readable for the callback duration.
    let status = unsafe { callback(host.context, bytes.as_ptr(), bytes.len()) };
    if status != STATUS_OK {
        return Err(format!(
            "host rejected the dynamic DLS catalog with status {status}"
        ));
    }
    Ok(())
}

fn resource_path(host: &HostApiV1) -> Result<PathBuf, String> {
    if version_major(host.api_version) != 1
        || version_minor(host.api_version) < 1
        || host.struct_size < size_of::<HostApiV1>() as u32
    {
        return Err("host does not provide RackForge resource API 1.1".into());
    }
    let callback = host
        .get_resource_path
        .ok_or_else(|| "host resource callback is unavailable".to_owned())?;
    // SAFETY: RESOURCE_ID is readable and a null destination queries the size.
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
        return Err("dls-bank resource is missing or invalid".into());
    }
    let mut bytes = vec![0_u8; required];
    // SAFETY: the destination has exactly the size reported by the host.
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
        return Err("dls-bank path changed while being read".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "dls-bank path is not UTF-8".to_owned())?;
    Ok(PathBuf::from(text))
}

fn addon_data_path(host: &HostApiV1) -> Result<Option<PathBuf>, String> {
    if version_major(host.api_version) != 1 || version_minor(host.api_version) < 2 {
        return Ok(None);
    }
    if host.struct_size < size_of::<HostApiV1>() as u32 {
        return Err("host API 1.2 table is truncated".into());
    }
    let Some(callback) = host.get_addon_data_path else {
        return Ok(None);
    };
    // SAFETY: a null destination queries the required path size.
    let required = unsafe { callback(host.context, ptr::null_mut(), 0) };
    if required == 0 {
        return Ok(None);
    }
    if required > 32 * 1024 {
        return Err("addon data path is unreasonably large".into());
    }
    let mut bytes = vec![0_u8; required];
    // SAFETY: bytes is writable for exactly the reported size.
    let reported = unsafe { callback(host.context, bytes.as_mut_ptr(), bytes.len()) };
    if reported != required {
        return Err("addon data path changed while being read".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "addon data path is not UTF-8".to_owned())?;
    Ok(Some(PathBuf::from(text)))
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
    let result = resource_path(host)
        .and_then(|path| DlsBank::open(path).map_err(|error| error.to_string()))
        .and_then(|bank| addon_data_path(host).map(|data_root| (bank, data_root)))
        .and_then(|(bank, data_root)| {
            let (mut custom_programs, warnings) =
                custom_program::load_programs(data_root.as_deref())?;
            for warning in warnings {
                log_host(host, LOG_LEVEL_WARN, &warning);
            }
            custom_programs.retain(|program| {
                let valid = bank
                    .instrument(program.source.bank, program.source.program)
                    .is_some();
                if !valid {
                    log_host(
                        host,
                        LOG_LEVEL_WARN,
                        &format!(
                            "ignoring CUSTOM {:?}: DLS bank {} program {} is missing",
                            program.id, program.source.bank, program.source.program
                        ),
                    );
                }
                valid
            });
            let default = bank
                .instrument(0, 0)
                .or_else(|| {
                    bank.instruments
                        .iter()
                        .find(|instrument| !instrument.is_drum())
                })
                .or_else(|| bank.instruments.first())
                .map(|instrument| (instrument.bank, instrument.program))
                .ok_or_else(|| "DLS bank contains no selectable instruments".to_owned())?;
            publish_dynamic_catalog(host, &bank, &custom_programs)?;
            Ok((bank, default, custom_programs))
        });
    match result {
        Ok((bank, (selected_bank, selected_program), custom_programs)) => {
            log_host(
                host,
                LOG_LEVEL_INFO,
                &format!(
                    "RF-DLS bank loaded with {} CUSTOM programs",
                    custom_programs.len()
                ),
            );
            Box::into_raw(Box::new(RfDls::new(
                bank,
                selected_bank,
                selected_program,
                custom_programs,
            )))
            .cast()
        }
        Err(error) => {
            log_host(host, LOG_LEVEL_ERROR, &error);
            ptr::null_mut()
        }
    }
}

unsafe extern "C" fn destroy(instance: *mut c_void) {
    if !instance.is_null() {
        // SAFETY: instances originate from Box::into_raw in create.
        unsafe {
            drop(Box::from_raw(instance.cast::<RfDls>()));
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
    let Some(plugin) = (unsafe { instance.cast::<RfDls>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if !sample_rate.is_finite()
        || !(8_000.0..=384_000.0).contains(&sample_rate)
        || maximum_frames == 0
        || input_channels != 0
        || output_channels == 0
        || output_channels > 8
    {
        return STATUS_INVALID_ARGUMENT;
    }
    plugin.sample_rate = sample_rate.round() as u32;
    plugin.maximum_frames = maximum_frames;
    plugin.output_channels = output_channels;
    plugin.active = true;
    plugin.reset();
    STATUS_OK
}

unsafe extern "C" fn deactivate(instance: *mut c_void) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RfDls>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    plugin.reset();
    plugin.active = false;
    STATUS_OK
}

unsafe extern "C" fn reset(instance: *mut c_void) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RfDls>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    plugin.reset();
    STATUS_OK
}

unsafe extern "C" fn set_parameter(instance: *mut c_void, parameter_index: u32, value: f64) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RfDls>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    plugin.set_parameter(parameter_index, value)
}

unsafe extern "C" fn get_parameter(
    instance: *mut c_void,
    parameter_index: u32,
    value: *mut f64,
) -> i32 {
    let (Some(plugin), Some(output)) = (unsafe { instance.cast::<RfDls>().as_ref() }, unsafe {
        value.as_mut()
    }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    *output = match parameter_index {
        MASTER_GAIN_PARAMETER => f64::from(plugin.master_gain),
        _ => return STATUS_UNKNOWN_PARAMETER,
    };
    STATUS_OK
}

fn state_bytes(plugin: &RfDls) -> Vec<u8> {
    let id = plugin.selected_preset_id.as_bytes();
    let length = u16::try_from(id.len()).expect("validated preset IDs fit in u16");
    let mut state = Vec::with_capacity(10 + id.len());
    state.extend_from_slice(b"RFD2");
    state.extend_from_slice(&plugin.master_gain.to_le_bytes());
    state.extend_from_slice(&length.to_le_bytes());
    state.extend_from_slice(id);
    state
}

unsafe extern "C" fn save_state(
    instance: *mut c_void,
    destination: *mut u8,
    capacity: usize,
) -> usize {
    let Some(plugin) = (unsafe { instance.cast::<RfDls>().as_ref() }) else {
        return 0;
    };
    let bytes = state_bytes(plugin);
    unsafe { copy_to_host_buffer(&bytes, destination, capacity) }
}

unsafe extern "C" fn load_state(instance: *mut c_void, source: *const u8, length: usize) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RfDls>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if source.is_null() || length < 4 {
        return STATUS_INVALID_ARGUMENT;
    }
    let bytes = unsafe { slice::from_raw_parts(source, length) };
    let (gain, selected) = if &bytes[..4] == b"RFD1" && length == LEGACY_STATE_SIZE {
        let gain = f32::from_le_bytes(bytes[4..8].try_into().expect("fixed slice"));
        let bank = u32::from_le_bytes(bytes[8..12].try_into().expect("fixed slice"));
        let program = u32::from_le_bytes(bytes[12..16].try_into().expect("fixed slice"));
        (gain, dynamic_preset_id(bank, program))
    } else if &bytes[..4] == b"RFD2" && length >= 10 {
        let gain = f32::from_le_bytes(bytes[4..8].try_into().expect("fixed slice"));
        let id_length = u16::from_le_bytes(bytes[8..10].try_into().expect("fixed slice")) as usize;
        if id_length == 0 || length != 10 + id_length {
            return STATUS_INVALID_ARGUMENT;
        }
        let Ok(id) = std::str::from_utf8(&bytes[10..]) else {
            return STATUS_INVALID_ARGUMENT;
        };
        (gain, id.to_owned())
    } else {
        return STATUS_INVALID_ARGUMENT;
    };
    let status = plugin.set_parameter(MASTER_GAIN_PARAMETER, f64::from(gain));
    if status != STATUS_OK {
        return status;
    }
    if !plugin.select_preset(&selected) {
        return STATUS_INVALID_STATE;
    }
    STATUS_OK
}

unsafe extern "C" fn load_preset(
    instance: *mut c_void,
    preset_id: *const u8,
    length: usize,
) -> i32 {
    let Some(plugin) = (unsafe { instance.cast::<RfDls>().as_mut() }) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if preset_id.is_null() || length == 0 {
        return STATUS_INVALID_ARGUMENT;
    }
    let id = unsafe { slice::from_raw_parts(preset_id, length) };
    let Ok(id) = std::str::from_utf8(id) else {
        return STATUS_INVALID_ARGUMENT;
    };
    if !plugin.select_preset(id) {
        return STATUS_INVALID_STATE;
    }
    plugin.master_gain = DEFAULT_MASTER_GAIN;
    STATUS_OK
}

unsafe extern "C" fn process(instance: *mut c_void, block: *const ProcessBlockV1) -> i32 {
    let (Some(plugin), Some(block)) = (unsafe { instance.cast::<RfDls>().as_mut() }, unsafe {
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
    api_version: pack_version(1, 3),
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
pub extern "C" fn rackforge_plugin_entry_v1() -> *const PluginApiV1 {
    ptr::addr_of!(PLUGIN_API)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rackforge_plugin_api::{ParameterSchema, PresetCatalog, RuntimeDescriptor};
    use rf_dls::{
        EnvelopeSpec, Instrument, LfoSpec, PitchEnvelopeSpec, Region, SampleLoop, SampleParams,
        Wave,
    };
    use std::sync::Arc;

    fn custom_program() -> CustomProgram {
        let document = serde_json::from_slice(include_bytes!(
            "../examples/custom.warm-piano.rackforge-program.json"
        ))
        .unwrap();
        CustomProgram::from_document(document).unwrap()
    }

    fn synthetic_plugin() -> RfDls {
        let samples = (0..512)
            .map(|frame| if frame % 2 == 0 { 0.5 } else { -0.5 })
            .collect::<Vec<_>>();
        RfDls::new(
            DlsBank {
                instruments: vec![Instrument {
                    name: "Synthetic Piano".into(),
                    bank: 0,
                    program: 0,
                    regions: vec![Region {
                        key_low: 0,
                        key_high: 127,
                        velocity_low: 0,
                        velocity_high: 127,
                        wave_index: 0,
                        key_group: 0,
                        sample_params: Some(SampleParams {
                            unity_note: 60,
                            fine_tune: 0,
                            attenuation_db: 0.0,
                            sample_loop: Some(SampleLoop { start: 0, end: 512 }),
                        }),
                    }],
                    envelope: EnvelopeSpec {
                        attack_seconds: 0.0,
                        decay_seconds: 1_000.0,
                        sustain_level: 1.0,
                        release_seconds: 0.01,
                    },
                    pitch_envelope: PitchEnvelopeSpec::default(),
                    lfo: LfoSpec::default(),
                }],
                waves: vec![Wave {
                    name: "Synthetic Wave".into(),
                    sample_rate: 48_000,
                    samples: Arc::from(samples),
                    sample_params: None,
                }],
            },
            0,
            0,
            Vec::new(),
        )
    }

    #[test]
    fn exports_valid_metadata() {
        let descriptor: RuntimeDescriptor = serde_json::from_slice(RUNTIME_DESCRIPTOR).unwrap();
        assert_eq!(descriptor.id, "org.rackforge.rf-dls");
        let parameters: ParameterSchema = serde_json::from_slice(PARAMETER_SCHEMA).unwrap();
        assert_eq!(parameters.validate(), Ok(()));
        let presets: PresetCatalog = serde_json::from_slice(PRESET_CATALOG).unwrap();
        assert_eq!(presets.validate(), Ok(()));
    }

    #[test]
    fn dynamic_catalog_uses_opaque_stable_ids() {
        let plugin = synthetic_plugin();
        let catalog = dynamic_catalog(&plugin.bank, &[]);
        assert_eq!(catalog.validate(), Ok(()));
        assert_eq!(catalog.presets.len(), 1);
        assert_eq!(catalog.presets[0].id, "dls.b00000000.p00000000");
        assert_eq!(catalog.presets[0].description.as_deref(), Some("B000 P000"));
        assert_eq!(
            parse_dynamic_preset_id(&catalog.presets[0].id),
            Some((0, 0))
        );
    }

    #[test]
    fn custom_programs_are_separate_and_survive_state_round_trip() {
        let mut plugin = synthetic_plugin();
        plugin.custom_programs.push(custom_program());
        let catalog = dynamic_catalog(&plugin.bank, &plugin.custom_programs);
        assert_eq!(catalog.validate(), Ok(()));
        assert_eq!(catalog.banks[0].name, "DLS");
        assert_eq!(catalog.banks[1].name, "CUSTOM");
        assert_eq!(catalog.presets[1].id, "custom.user.warm-piano");
        assert_eq!(catalog.presets[1].bank.as_deref(), Some("custom"));

        assert!(plugin.select_custom("custom.user.warm-piano"));
        let state = state_bytes(&plugin);
        assert!(plugin.select_instrument(0, 0));
        let status = unsafe {
            load_state(
                (&mut plugin as *mut RfDls).cast(),
                state.as_ptr(),
                state.len(),
            )
        };
        assert_eq!(status, STATUS_OK);
        assert_eq!(plugin.selected_preset_id, "custom.user.warm-piano");
        assert_eq!(plugin.envelope_override.unwrap().release_seconds, 1.2);
    }

    #[test]
    fn note_on_and_note_off_drive_a_voice() {
        let mut plugin = synthetic_plugin();
        plugin.note_on(60, 127);
        assert_eq!(plugin.voices.len(), 1);
        assert_ne!(plugin.render_frame(), 0.0);
        plugin.note_off(60);
        assert!(!plugin.held_notes[60]);
        for _ in 0..600 {
            plugin.render_frame();
        }
        assert!(plugin.voices.is_empty());
    }

    #[test]
    fn sustain_defers_release_until_the_pedal_lifts() {
        let mut plugin = synthetic_plugin();
        plugin.note_on(60, 100);
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xb0, 64, 127],
        });
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0x80, 60, 0],
        });
        for _ in 0..600 {
            plugin.render_frame();
        }
        assert_eq!(plugin.voices.len(), 1);
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xb0, 64, 0],
        });
        for _ in 0..600 {
            plugin.render_frame();
        }
        assert!(plugin.voices.is_empty());
    }

    #[test]
    fn all_notes_off_respects_sustain_but_all_sound_off_does_not() {
        let mut plugin = synthetic_plugin();
        plugin.note_on(60, 100);
        plugin.set_sustain(true);
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xb0, 123, 0],
        });
        assert_eq!(plugin.voices.len(), 1);
        assert!(!plugin.held_notes[60]);
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xb0, 120, 0],
        });
        assert!(plugin.voices.is_empty());
        assert!(!plugin.sustain);
    }

    #[test]
    fn midi_note_on_with_zero_velocity_is_note_off() {
        let mut plugin = synthetic_plugin();
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0x90, 60, 127],
        });
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0x90, 60, 0],
        });
        assert!(!plugin.held_notes[60]);
    }

    #[test]
    fn pitch_bend_uses_the_full_fourteen_bit_range() {
        let mut plugin = synthetic_plugin();
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xe0, 0, 0],
        });
        assert_eq!(plugin.pitch_bend_cents, -200.0);
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xe0, 0, 64],
        });
        assert_eq!(plugin.pitch_bend_cents, 0.0);
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xe0, 127, 127],
        });
        assert_eq!(plugin.pitch_bend_cents, 200.0);
    }

    #[test]
    fn mod_wheel_and_reset_controllers_update_plugin_state() {
        let mut plugin = synthetic_plugin();
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xb0, 1, 127],
        });
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xe0, 127, 127],
        });
        assert_eq!(plugin.modulation_wheel, 1.0);
        assert_eq!(plugin.pitch_bend_cents, 200.0);
        plugin.handle_midi(MidiEventV1 {
            frame: 0,
            length: 3,
            data: [0xb0, 121, 0],
        });
        assert_eq!(plugin.modulation_wheel, 0.0);
        assert_eq!(plugin.pitch_bend_cents, 0.0);
    }

    #[test]
    fn state_round_trip_preserves_gain_and_program() {
        let mut plugin = synthetic_plugin();
        plugin.master_gain = 0.25;
        let state = state_bytes(&plugin);
        plugin.master_gain = 1.0;
        let status = unsafe {
            load_state(
                (&mut plugin as *mut RfDls).cast(),
                state.as_ptr(),
                state.len(),
            )
        };
        assert_eq!(status, STATUS_OK);
        assert_eq!(plugin.master_gain, 0.25);
    }
}
