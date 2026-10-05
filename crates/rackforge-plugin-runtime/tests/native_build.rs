#![cfg(not(target_arch = "wasm32"))]

//! A native build behind `PortableInstance`, end to end.
//!
//! The processor is exported by the SDK into this test binary and loaded by
//! its portable entry, as a host loads a library's: what is tested is the
//! runtime's side -- the calling sequence, the checks a component gets, the
//! regions, and the two threads -- through the same type `rackforge-core`
//! holds. That a native build and its component answer alike is the SDK's
//! test (`native_portable.rs`); that a packaged plugin renders the same
//! either way is the core's.

use std::sync::atomic::{AtomicUsize, Ordering};

use rackforge_plugin_runtime::{MidiEvent, MidiEvent2, ParameterEvent, PortableModule};
use rackforge_plugin_sdk::portable::native::NativeApiV1;
use rackforge_plugin_sdk::{
    MIDI_FAMILY_NOTE, PROGRAM_EDIT_BASIC, PROGRAM_EDIT_DECLARATIVE, PROGRAM_EDIT_PREVIEW, Processor,
};

static DROPPED: AtomicUsize = AtomicUsize::new(0);

/// More than any audio thread's stack, built by value where a processor
/// builds its engine: in `prepare`.
const TABLE: usize = 1 << 20;

struct Probe {
    gain: f64,
    table_sum: f32,
    resource: Vec<u8>,
    resource_expected: u64,
}

impl Default for Probe {
    fn default() -> Self {
        Self {
            gain: 0.5,
            table_sum: 0.0,
            resource: Vec::new(),
            resource_expected: 0,
        }
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

#[inline(never)]
fn build_table() -> [f32; TABLE] {
    let mut table = [0.0_f32; TABLE];
    for (index, value) in table.iter_mut().enumerate() {
        *value = (index % 7) as f32;
    }
    std::hint::black_box(table)
}

impl Processor for Probe {
    fn prepare(&mut self, _rate: f64, _frames: u32, _inputs: u32, _outputs: u32) -> bool {
        let table = build_table();
        self.table_sum = table[..16].iter().sum();
        true
    }

    fn set_parameter(&mut self, index: u32, value: f64) -> bool {
        if index != 0 {
            return false;
        }
        self.gain = value;
        true
    }

    fn get_parameter(&self, index: u32) -> Option<f64> {
        (index == 0).then_some(self.gain)
    }

    fn latency_frames(&self) -> u32 {
        7
    }

    fn reset(&mut self) {
        self.gain = 0.5;
    }

    fn set_realtime_budget(&mut self, _fuel: u64) -> bool {
        true
    }

    fn begin_resource(&mut self, id: &str, total_bytes: u64) -> bool {
        self.resource.clear();
        self.resource_expected = total_bytes;
        id == "samples"
    }

    fn write_resource(&mut self, offset: u64, bytes: &[u8]) -> bool {
        if offset != self.resource.len() as u64 {
            return false;
        }
        self.resource.extend_from_slice(bytes);
        true
    }

    fn end_resource(&mut self) -> bool {
        self.resource.len() as u64 == self.resource_expected
    }

    fn write_program_catalog(&mut self, destination: &mut [u8]) -> Option<usize> {
        let catalog = b"{\"programs\":[]}";
        destination
            .get_mut(..catalog.len())?
            .copy_from_slice(catalog);
        Some(catalog.len())
    }

    fn load_preset(&mut self, id: &str) -> bool {
        if id != "bright" {
            return false;
        }
        self.gain = 1.0;
        true
    }

    fn save_state(&self, destination: &mut [u8]) -> Option<usize> {
        destination
            .get_mut(..8)?
            .copy_from_slice(&self.gain.to_le_bytes());
        Some(8)
    }

    fn load_state(&mut self, state: &[u8]) -> bool {
        let Ok(bytes) = <[u8; 8]>::try_from(state) else {
            return false;
        };
        self.gain = f64::from_le_bytes(bytes);
        true
    }

    fn program_editing_capabilities(&self) -> u32 {
        PROGRAM_EDIT_BASIC | PROGRAM_EDIT_PREVIEW | PROGRAM_EDIT_DECLARATIVE
    }

    fn begin_program_edit(&mut self, request: &[u8], destination: &mut [u8]) -> Option<usize> {
        let answer = destination.get_mut(..request.len())?;
        answer.copy_from_slice(request);
        answer.reverse();
        Some(request.len())
    }

    fn prepare_program_save(&mut self, document: &[u8], destination: &mut [u8]) -> Option<usize> {
        let answer = destination.get_mut(..document.len())?;
        answer.copy_from_slice(document);
        answer.make_ascii_uppercase();
        Some(document.len())
    }

    fn install_program(&mut self, prepared: &[u8]) -> bool {
        let Some(&first) = prepared.first() else {
            return false;
        };
        self.gain = f64::from(first) / 100.0;
        true
    }

    fn preview_program(&mut self, prepared: &[u8]) -> bool {
        !prepared.is_empty()
    }

    fn program_editor_view(&mut self, document: &[u8], destination: &mut [u8]) -> Option<usize> {
        let answer = destination.get_mut(..document.len())?;
        answer.copy_from_slice(document);
        Some(document.len())
    }

    fn apply_program_edit(&mut self, request: &[u8], destination: &mut [u8]) -> Option<usize> {
        *destination.first_mut()? = request.len() as u8;
        Some(1)
    }

    fn process(
        &mut self,
        _input: &[f32],
        output: &mut [f32],
        _midi: &[rackforge_plugin_sdk::MidiEvent],
        _parameters: &[rackforge_plugin_sdk::ParameterEvent],
        _frames: u32,
        _input_channels: u32,
        _output_channels: u32,
    ) {
        output.fill(-1.0);
    }

    /// Input times the gain (automation at its frame), plus each narrow
    /// event's status byte and each wide event's 16-bit velocity at theirs.
    fn process_wide(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        midi: &[rackforge_plugin_sdk::MidiEvent],
        midi2: &[rackforge_plugin_sdk::MidiEvent2],
        parameters: &[rackforge_plugin_sdk::ParameterEvent],
        frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) {
        for frame in 0..frames as usize {
            for event in parameters
                .iter()
                .filter(|event| event.frame as usize == frame)
            {
                self.gain = event.value;
            }
            let marker: f32 = midi
                .iter()
                .filter(|event| event.frame as usize == frame)
                .map(|event| f32::from(event.data[0]))
                .sum::<f32>()
                + midi2
                    .iter()
                    .filter(|event| event.frame as usize == frame)
                    .map(|event| event.value as f32)
                    .sum::<f32>();
            for channel in 0..output_channels as usize {
                let source =
                    input[frame * input_channels as usize + channel % input_channels as usize];
                output[frame * output_channels as usize + channel] =
                    (f64::from(source) * self.gain) as f32 + marker + self.table_sum;
            }
        }
    }
}

rackforge_plugin_sdk::export_processor!(
    Probe,
    max_frames = 64,
    max_input_channels = 2,
    max_output_channels = 2,
    max_midi_events = 4,
    max_parameter_events = 4,
    max_transfer_bytes = 32,
    midi2 = { max_events = 2, families = MIDI_FAMILY_NOTE }
);

unsafe extern "C" {
    fn rackforge_portable_native_entry_v1() -> *const NativeApiV1;
}

/// A thread with an audio thread's stack, or less: 256 KiB.
fn on_small_stack<T: Send>(work: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(256 << 10)
            .spawn_scoped(scope, work)
            .unwrap()
            .join()
            .unwrap()
    })
}

fn module() -> PortableModule {
    unsafe { PortableModule::from_native_entry(rackforge_portable_native_entry_v1) }.unwrap()
}

#[test]
fn a_native_build_runs_behind_the_portable_instance() {
    let module = module();
    assert!(module.is_native_build());

    // Made and prepared from a thread far too small for the table `prepare`
    // builds: the control thread builds it.
    let mut instance = on_small_stack(|| {
        let mut instance = module.instantiate().unwrap();
        assert!(
            instance
                .process_interleaved(&[0.0; 16], &mut [0.0; 16], 8)
                .is_err()
        );
        instance.prepare(48_000.0, 32, 2, 2).unwrap();
        instance
    });
    assert!(instance.is_native_build());
    assert!(
        instance.prepare(48_000.0, 65, 2, 2).is_err(),
        "beyond the output region"
    );
    assert!(
        instance.prepare(48_000.0, 32, 3, 2).is_err(),
        "more inputs than declared"
    );

    // What only the sandbox has: no fuel, no budget, no parallel units --
    // even though this processor would take a budget.
    assert!(!instance.is_metered());
    assert!(!instance.accepts_realtime_budget());
    assert!(!instance.set_realtime_budget(1_000_000).unwrap());
    assert_eq!(instance.last_realtime_fuel_consumed(), 0);
    assert!(instance.parallel_layout().is_none());
    assert!(instance.parallel_end_block(&mut [0.0; 4], 2).is_err());

    assert_eq!(instance.latency_frames().unwrap(), 7);
    assert_eq!(instance.midi2_families(), MIDI_FAMILY_NOTE);
    instance.set_parameter(0, 0.25).unwrap();
    assert_eq!(instance.get_parameter(0).unwrap(), 0.25);
    assert!(instance.set_parameter(9, 1.0).is_err());
    assert!(instance.get_parameter(9).is_err());

    // A block on the small stack: the wide entry, every kind of event.
    let input: Vec<f32> = (0..16).map(|i| i as f32).collect();
    let midi = [MidiEvent::new(1, &[0x90, 60, 100]).unwrap()];
    let midi2 = [MidiEvent2 {
        frame: 2,
        kind: 1,
        channel: 0,
        index: 60,
        flags: 0,
        value: 1000,
        extra: 0,
    }];
    let automation = [ParameterEvent {
        frame: 4,
        index: 0,
        value: 2.0,
    }];
    let (instance, output) = on_small_stack(move || {
        let mut output = vec![0.0_f32; 16];
        instance
            .process_interleaved_with_midi2(&input, &mut output, 8, &midi, &automation, &midi2)
            .unwrap();
        (instance, output)
    });
    let mut instance = instance;
    let table_sum: f32 = (0..16).map(|i| (i % 7) as f32).sum();
    let expected: Vec<f32> = (0..16)
        .map(|i| {
            let frame = i / 2;
            let gain = if frame >= 4 { 2.0 } else { 0.25 };
            let marker = match frame {
                1 => 144.0,
                2 => 1000.0,
                _ => 0.0,
            };
            (f64::from(i as f32) * gain) as f32 + marker + table_sum
        })
        .collect();
    assert_eq!(output, expected);

    // The component's refusals, before the processor runs.
    let mut output = vec![0.0_f32; 16];
    assert!(
        instance
            .process_interleaved(&[0.0; 16], &mut output, 33)
            .is_err()
    );
    assert!(
        instance
            .process_interleaved(&[0.0; 14], &mut output, 8)
            .is_err()
    );
    let late = [MidiEvent::new(8, &[0x90, 60, 1]).unwrap()];
    assert!(
        instance
            .process_interleaved_with_midi(&[0.0; 16], &mut output, 8, &late)
            .is_err()
    );
    let too_wide = [midi2[0]; 3];
    assert!(
        instance
            .process_interleaved_with_midi2(&[0.0; 16], &mut output, 8, &[], &[], &too_wide)
            .is_err()
    );

    // State, presets and the catalog through the transfer region.
    let state = instance.save_state().unwrap();
    assert_eq!(state, 2.0_f64.to_le_bytes());
    let mut other = module.instantiate().unwrap();
    other.load_state(&state).unwrap();
    assert_eq!(other.get_parameter(0).unwrap(), 2.0);
    assert!(other.load_state(&[1, 2, 3]).is_err());
    assert!(
        other.load_state(&[0; 33]).is_err(),
        "beyond the transfer region"
    );
    other.load_preset("bright").unwrap();
    assert_eq!(other.get_parameter(0).unwrap(), 1.0);
    assert!(other.load_preset("dull").is_err());
    assert!(other.load_preset("").is_err());
    assert_eq!(
        other.preset_catalog().unwrap().as_deref(),
        Some(&b"{\"programs\":[]}"[..])
    );
    // The instances share nothing.
    assert_eq!(instance.get_parameter(0).unwrap(), 2.0);
    instance.reset().unwrap();
    assert_eq!(instance.get_parameter(0).unwrap(), 0.5);
    assert_eq!(other.get_parameter(0).unwrap(), 1.0);

    // A resource larger than the transfer region arrives in chunks.
    let resource: Vec<u8> = (0..100).collect();
    other.load_resource("samples", &resource).unwrap();
    assert!(other.load_resource("unknown", &resource).is_err());

    // Program editing, every capability.
    assert!(other.supports_program_editing());
    assert_eq!(other.begin_program_edit(b"abc").unwrap(), b"cba");
    assert_eq!(other.prepare_program_save(b"abc").unwrap(), b"ABC");
    assert_eq!(other.program_editor_view(b"view").unwrap(), b"view");
    assert_eq!(other.apply_program_edit(b"12345").unwrap(), [5]);
    assert!(other.preview_program(b"x").unwrap());
    other.install_program(&[50]).unwrap();
    assert_eq!(other.get_parameter(0).unwrap(), 0.5);
    assert!(other.install_program(&[]).is_err());
    assert!(
        other.begin_program_edit(&[0; 33]).is_err(),
        "beyond the transfer region"
    );

    // Each instance's processor is dropped with it.
    let before = DROPPED.load(Ordering::SeqCst);
    drop(other);
    drop(instance);
    assert_eq!(DROPPED.load(Ordering::SeqCst), before + 2);
}

#[test]
fn a_library_that_is_not_there_is_refused() {
    let missing = std::env::temp_dir().join("rackforge-no-such-native-build.bin");
    let error = unsafe { PortableModule::load_native_build(&missing) }
        .err()
        .expect("nothing to load");
    assert!(format!("{error:#}").contains("native plugin build"));
}
