//! Sandboxed host for RackForge `wasm-v1` processors.

use anyhow::{Context, Result, bail};
use std::ops::Range;
use std::path::Path;
use wasmtime::{
    Cache, CacheConfig, Config, Engine, Instance, Memory, Module, OptLevel, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};

pub const ABI_VERSION_V1: i32 = 0x0001_0001;
const STATUS_OK: i32 = 0;

#[derive(Clone, Copy, Debug)]
pub struct RuntimeLimits {
    pub maximum_memory_bytes: usize,
    pub fuel_per_call: u64,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            maximum_memory_bytes: 64 * 1024 * 1024,
            fuel_per_call: 50_000_000,
        }
    }
}

struct HostState {
    limits: StoreLimits,
}

pub struct PortableEngine {
    engine: Engine,
    limits: RuntimeLimits,
}

impl PortableEngine {
    pub fn new(limits: RuntimeLimits) -> Result<Self> {
        Self::configured(limits, None)
    }

    /// Creates a compiler whose derived native code is cached below a
    /// RackForge-owned directory. The cache is disposable and never belongs in
    /// the plugin package.
    pub fn with_cache(limits: RuntimeLimits, directory: impl AsRef<Path>) -> Result<Self> {
        let directory = directory.as_ref();
        std::fs::create_dir_all(directory)
            .with_context(|| format!("creating portable code cache {}", directory.display()))?;
        let directory = std::fs::canonicalize(directory)
            .with_context(|| format!("resolving portable code cache {}", directory.display()))?;
        Self::configured(limits, Some(&directory))
    }

    fn configured(limits: RuntimeLimits, cache_directory: Option<&Path>) -> Result<Self> {
        let mut config = Config::new();
        config.cranelift_opt_level(OptLevel::Speed);
        config.consume_fuel(true);
        config.wasm_multi_memory(false);
        config.wasm_memory64(false);
        if let Some(directory) = cache_directory {
            let mut cache_config = CacheConfig::new();
            cache_config.with_directory(directory);
            config.cache(Some(Cache::new(cache_config).map_err(|error| {
                anyhow::anyhow!("creating RackForge portable code cache: {error}")
            })?));
        }
        Ok(Self {
            engine: Engine::new(&config).map_err(|error| {
                anyhow::anyhow!("creating RackForge WebAssembly engine: {error}")
            })?,
            limits,
        })
    }

    pub fn compile(&self, bytes: &[u8]) -> Result<PortableModule> {
        let module = Module::from_binary(&self.engine, bytes).map_err(|error| {
            anyhow::anyhow!("compiling RackForge WebAssembly component: {error}")
        })?;
        if let Some(import) = module.imports().next() {
            bail!(
                "wasm-v1 modules may not import host functions (found {}::{})",
                import.module(),
                import.name()
            );
        }
        Ok(PortableModule {
            module,
            limits: self.limits,
        })
    }
}

pub struct PortableModule {
    module: Module,
    limits: RuntimeLimits,
}

impl PortableModule {
    pub fn instantiate(&self) -> Result<PortableInstance> {
        let store_limits = StoreLimitsBuilder::new()
            .memory_size(self.limits.maximum_memory_bytes)
            .memories(1)
            .instances(1)
            .build();
        let mut store = Store::new(
            self.module.engine(),
            HostState {
                limits: store_limits,
            },
        );
        store.limiter(|state| &mut state.limits);
        store.set_fuel(self.limits.fuel_per_call)?;
        let instance = Instance::new(&mut store, &self.module, &[]).map_err(|error| {
            anyhow::anyhow!("instantiating RackForge WebAssembly plugin: {error}")
        })?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .context("wasm-v1 plugin does not export memory")?;
        let abi_version = typed::<(), i32>(&instance, &mut store, "rackforge_abi_version")?;
        let version = abi_version.call(&mut store, ())?;
        if version != ABI_VERSION_V1 {
            bail!("unsupported wasm-v1 ABI version {version:#010x}");
        }
        let input_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_input_ptr")?;
        let output_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_output_ptr")?;
        let midi_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_midi_ptr")?;
        let parameter_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_parameter_ptr")?;
        let transfer_ptr = typed::<(), i32>(&instance, &mut store, "rackforge_transfer_ptr")?;
        let input_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_input_samples")?
                .call(&mut store, ())?;
        let output_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_output_samples")?
                .call(&mut store, ())?;
        if input_capacity < 0 || output_capacity < 0 {
            bail!("wasm-v1 plugin reported an invalid audio buffer capacity");
        }
        let midi_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_midi_events")?
                .call(&mut store, ())?;
        if midi_capacity <= 0 {
            bail!("wasm-v1 plugin reported an invalid MIDI event capacity");
        }
        let parameter_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_parameter_events")?
                .call(&mut store, ())?;
        if parameter_capacity <= 0 {
            bail!("wasm-v1 plugin reported an invalid parameter event capacity");
        }
        let transfer_capacity =
            typed::<(), i32>(&instance, &mut store, "rackforge_capacity_transfer_bytes")?
                .call(&mut store, ())?;
        if transfer_capacity <= 0 {
            bail!("wasm-v1 plugin reported an invalid transfer capacity");
        }
        let input_offset = input_ptr.call(&mut store, ())?;
        let output_offset = output_ptr.call(&mut store, ())?;
        let midi_offset = midi_ptr.call(&mut store, ())?;
        let parameter_offset = parameter_ptr.call(&mut store, ())?;
        let transfer_offset = transfer_ptr.call(&mut store, ())?;
        let prepare = typed(&instance, &mut store, "rackforge_prepare")?;
        let set_parameter = typed(&instance, &mut store, "rackforge_set_parameter")?;
        let get_parameter = typed(&instance, &mut store, "rackforge_get_parameter")?;
        let reset = typed(&instance, &mut store, "rackforge_reset")?;
        let resource_begin = typed(&instance, &mut store, "rackforge_resource_begin")?;
        let resource_write = typed(&instance, &mut store, "rackforge_resource_write")?;
        let resource_end = typed(&instance, &mut store, "rackforge_resource_end")?;
        let load_preset = typed(&instance, &mut store, "rackforge_load_preset")?;
        let save_state = typed(&instance, &mut store, "rackforge_save_state")?;
        let load_state = typed(&instance, &mut store, "rackforge_load_state")?;
        let process = typed(&instance, &mut store, "rackforge_process")?;
        Ok(PortableInstance {
            store,
            memory,
            input_offset,
            output_offset,
            midi_offset,
            parameter_offset,
            transfer_offset,
            capacity_input_samples: input_capacity as usize,
            capacity_output_samples: output_capacity as usize,
            capacity_midi_events: midi_capacity as usize,
            capacity_parameter_events: parameter_capacity as usize,
            capacity_transfer_bytes: transfer_capacity as usize,
            prepare,
            set_parameter,
            get_parameter,
            reset,
            resource_begin,
            resource_write,
            resource_end,
            load_preset,
            save_state,
            load_state,
            process,
            fuel_per_call: self.limits.fuel_per_call,
            prepared_input_channels: 0,
            prepared_output_channels: 0,
            maximum_frames: 0,
        })
    }
}

pub struct PortableInstance {
    store: Store<HostState>,
    memory: Memory,
    input_offset: i32,
    output_offset: i32,
    midi_offset: i32,
    parameter_offset: i32,
    transfer_offset: i32,
    capacity_input_samples: usize,
    capacity_output_samples: usize,
    capacity_midi_events: usize,
    capacity_parameter_events: usize,
    capacity_transfer_bytes: usize,
    prepare: TypedFunc<(f64, i32, i32, i32), i32>,
    set_parameter: TypedFunc<(i32, f64), i32>,
    get_parameter: TypedFunc<i32, f64>,
    reset: TypedFunc<(), i32>,
    resource_begin: TypedFunc<(i32, i64), i32>,
    resource_write: TypedFunc<(i64, i32), i32>,
    resource_end: TypedFunc<(), i32>,
    load_preset: TypedFunc<i32, i32>,
    save_state: TypedFunc<(), i32>,
    load_state: TypedFunc<i32, i32>,
    process: TypedFunc<(i32, i32, i32, i32, i32), i32>,
    fuel_per_call: u64,
    prepared_input_channels: u32,
    prepared_output_channels: u32,
    maximum_frames: u32,
}

impl PortableInstance {
    pub fn prepare(
        &mut self,
        sample_rate: f64,
        maximum_frames: u32,
        input_channels: u32,
        output_channels: u32,
    ) -> Result<()> {
        let input_samples = checked_samples(maximum_frames, input_channels)?;
        let output_samples = checked_samples(maximum_frames, output_channels)?;
        if input_samples > self.capacity_input_samples {
            bail!(
                "plugin input capacity {} samples is smaller than requested {input_samples}",
                self.capacity_input_samples
            );
        }
        if output_samples > self.capacity_output_samples {
            bail!(
                "plugin output capacity {} samples is smaller than requested {output_samples}",
                self.capacity_output_samples
            );
        }
        self.reset_fuel()?;
        check_status(
            self.prepare.call(
                &mut self.store,
                (
                    sample_rate,
                    maximum_frames as i32,
                    input_channels as i32,
                    output_channels as i32,
                ),
            )?,
            "prepare",
        )?;
        self.prepared_input_channels = input_channels;
        self.prepared_output_channels = output_channels;
        self.maximum_frames = maximum_frames;
        Ok(())
    }

    pub fn set_parameter(&mut self, index: u32, value: f64) -> Result<()> {
        self.reset_fuel()?;
        check_status(
            self.set_parameter
                .call(&mut self.store, (index as i32, value))?,
            "set_parameter",
        )
    }

    pub fn get_parameter(&mut self, index: u32) -> Result<f64> {
        self.reset_fuel()?;
        let value = self.get_parameter.call(&mut self.store, index as i32)?;
        if !value.is_finite() {
            bail!("portable plugin does not expose parameter {index}");
        }
        Ok(value)
    }

    pub fn reset(&mut self) -> Result<()> {
        self.reset_fuel()?;
        check_status(self.reset.call(&mut self.store, ())?, "reset")
    }

    /// Delivers one already-authorized package resource without exposing the
    /// filesystem to the guest. This method belongs on the control thread.
    pub fn load_resource(&mut self, id: &str, bytes: &[u8]) -> Result<()> {
        if id.is_empty() || id.len() > self.capacity_transfer_bytes {
            bail!("resource id does not fit the portable transfer buffer");
        }
        self.write_transfer(id.as_bytes())?;
        self.reset_fuel()?;
        check_status(
            self.resource_begin
                .call(&mut self.store, (id.len() as i32, bytes.len() as i64))?,
            "resource_begin",
        )?;
        for (chunk_index, chunk) in bytes.chunks(self.capacity_transfer_bytes).enumerate() {
            self.write_transfer(chunk)?;
            let offset = chunk_index
                .checked_mul(self.capacity_transfer_bytes)
                .context("resource offset overflow")?;
            self.reset_fuel()?;
            check_status(
                self.resource_write
                    .call(&mut self.store, (offset as i64, chunk.len() as i32))?,
                "resource_write",
            )?;
        }
        self.reset_fuel()?;
        check_status(self.resource_end.call(&mut self.store, ())?, "resource_end")
    }

    pub fn load_preset(&mut self, id: &str) -> Result<()> {
        if id.is_empty() {
            bail!("preset id must not be empty");
        }
        self.write_transfer(id.as_bytes())?;
        self.reset_fuel()?;
        check_status(
            self.load_preset.call(&mut self.store, id.len() as i32)?,
            "load_preset",
        )
    }

    pub fn save_state(&mut self) -> Result<Vec<u8>> {
        self.reset_fuel()?;
        let length = self.save_state.call(&mut self.store, ())?;
        if length < 0 || length as usize > self.capacity_transfer_bytes {
            bail!("portable plugin returned an invalid state length {length}");
        }
        Ok(self.read_transfer(length as usize)?.to_vec())
    }

    pub fn load_state(&mut self, state: &[u8]) -> Result<()> {
        self.write_transfer(state)?;
        self.reset_fuel()?;
        check_status(
            self.load_state.call(&mut self.store, state.len() as i32)?,
            "load_state",
        )
    }

    pub fn process_interleaved(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
    ) -> Result<()> {
        self.process_interleaved_with_events(input, output, frames, &[], &[])
    }

    pub fn process_interleaved_with_midi(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
        midi: &[MidiEvent],
    ) -> Result<()> {
        self.process_interleaved_with_events(input, output, frames, midi, &[])
    }

    pub fn process_interleaved_with_events(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        frames: u32,
        midi: &[MidiEvent],
        parameters: &[ParameterEvent],
    ) -> Result<()> {
        if frames == 0 || frames > self.maximum_frames {
            bail!("portable plugin is not prepared for this audio block");
        }
        let input_samples = checked_samples(frames, self.prepared_input_channels)?;
        let output_samples = checked_samples(frames, self.prepared_output_channels)?;
        if input.len() != input_samples || output.len() != output_samples {
            bail!("audio buffer length does not match prepared input/output channels");
        }
        if midi.len() > self.capacity_midi_events {
            bail!("MIDI event count exceeds plugin capacity");
        }
        if parameters.len() > self.capacity_parameter_events {
            bail!("parameter event count exceeds plugin capacity");
        }
        if midi
            .iter()
            .any(|event| event.frame >= frames || event.length == 0 || event.length > 3)
        {
            bail!("MIDI event is outside the audio block or has an invalid length");
        }
        if parameters
            .iter()
            .any(|event| event.frame >= frames || !event.value.is_finite())
            || parameters
                .windows(2)
                .any(|events| events[0].frame > events[1].frame)
        {
            bail!("parameter events must be finite, ordered, and inside the audio block");
        }
        let input_range = memory_range(
            self.input_offset,
            input_samples,
            self.memory.data_size(&self.store),
        )?;
        let output_range = memory_range(
            self.output_offset,
            output_samples,
            self.memory.data_size(&self.store),
        )?;
        let midi_range = byte_range(
            self.midi_offset,
            midi.len(),
            size_of::<u64>(),
            align_of::<u64>(),
            self.memory.data_size(&self.store),
        )?;
        let parameter_range = byte_range(
            self.parameter_offset,
            parameters.len(),
            16,
            align_of::<u64>(),
            self.memory.data_size(&self.store),
        )?;
        write_f32(self.memory.data_mut(&mut self.store), input_range, input);
        write_midi(self.memory.data_mut(&mut self.store), midi_range, midi);
        write_parameters(
            self.memory.data_mut(&mut self.store),
            parameter_range,
            parameters,
        );
        self.reset_fuel()?;
        check_status(
            self.process.call(
                &mut self.store,
                (
                    frames as i32,
                    self.prepared_input_channels as i32,
                    self.prepared_output_channels as i32,
                    midi.len() as i32,
                    parameters.len() as i32,
                ),
            )?,
            "process",
        )?;
        read_f32(self.memory.data(&self.store), output_range, output);
        Ok(())
    }

    fn reset_fuel(&mut self) -> Result<()> {
        self.store.set_fuel(self.fuel_per_call)?;
        Ok(())
    }

    fn write_transfer(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > self.capacity_transfer_bytes {
            bail!("control payload exceeds plugin transfer capacity");
        }
        let range = byte_range(
            self.transfer_offset,
            bytes.len(),
            1,
            1,
            self.memory.data_size(&self.store),
        )?;
        self.memory.data_mut(&mut self.store)[range].copy_from_slice(bytes);
        Ok(())
    }

    fn read_transfer(&self, length: usize) -> Result<&[u8]> {
        let range = byte_range(
            self.transfer_offset,
            length,
            1,
            1,
            self.memory.data_size(&self.store),
        )?;
        Ok(&self.memory.data(&self.store)[range])
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MidiEvent {
    pub frame: u32,
    pub data: [u8; 3],
    pub length: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParameterEvent {
    pub frame: u32,
    pub index: u32,
    pub value: f64,
}

impl MidiEvent {
    pub fn new(frame: u32, data: &[u8]) -> Result<Self> {
        if data.is_empty() || data.len() > 3 {
            bail!("real-time MIDI messages must contain one to three bytes");
        }
        let mut bytes = [0; 3];
        bytes[..data.len()].copy_from_slice(data);
        Ok(Self {
            frame,
            data: bytes,
            length: data.len() as u8,
        })
    }

    fn packed(self) -> u64 {
        self.frame as u64
            | (self.data[0] as u64) << 32
            | (self.data[1] as u64) << 40
            | (self.data[2] as u64) << 48
            | (self.length as u64) << 56
    }
}

fn typed<Params, Results>(
    instance: &Instance,
    store: &mut Store<HostState>,
    name: &str,
) -> Result<TypedFunc<Params, Results>>
where
    Params: wasmtime::WasmParams,
    Results: wasmtime::WasmResults,
{
    instance
        .get_typed_func(store, name)
        .map_err(|error| anyhow::anyhow!("wasm-v1 plugin is missing export {name}: {error}"))
}

fn checked_samples(frames: u32, channels: u32) -> Result<usize> {
    if frames == 0 {
        bail!("frames must be non-zero");
    }
    (frames as usize)
        .checked_mul(channels as usize)
        .context("audio sample count overflow")
}

fn memory_range(offset: i32, samples: usize, memory_size: usize) -> Result<Range<usize>> {
    byte_range(
        offset,
        samples,
        size_of::<f32>(),
        align_of::<f32>(),
        memory_size,
    )
}

fn byte_range(
    offset: i32,
    items: usize,
    item_size: usize,
    alignment: usize,
    memory_size: usize,
) -> Result<Range<usize>> {
    if offset < 0 || offset as usize % alignment != 0 {
        bail!("plugin returned an invalid audio buffer pointer");
    }
    let start = offset as usize;
    let bytes = items
        .checked_mul(item_size)
        .context("buffer byte count overflow")?;
    let end = start.checked_add(bytes).context("audio pointer overflow")?;
    if end > memory_size {
        bail!("plugin audio buffer escapes linear memory");
    }
    Ok(start..end)
}

fn write_midi(memory: &mut [u8], range: Range<usize>, events: &[MidiEvent]) {
    for (chunk, event) in memory[range].chunks_exact_mut(8).zip(events) {
        chunk.copy_from_slice(&event.packed().to_le_bytes());
    }
}

fn write_parameters(memory: &mut [u8], range: Range<usize>, events: &[ParameterEvent]) {
    for (chunk, event) in memory[range].chunks_exact_mut(16).zip(events) {
        chunk[0..4].copy_from_slice(&event.frame.to_le_bytes());
        chunk[4..8].copy_from_slice(&event.index.to_le_bytes());
        chunk[8..16].copy_from_slice(&event.value.to_le_bytes());
    }
}

fn write_f32(memory: &mut [u8], range: Range<usize>, samples: &[f32]) {
    for (chunk, sample) in memory[range].chunks_exact_mut(4).zip(samples) {
        chunk.copy_from_slice(&sample.to_le_bytes());
    }
}

fn read_f32(memory: &[u8], range: Range<usize>, samples: &mut [f32]) {
    for (sample, chunk) in samples.iter_mut().zip(memory[range].chunks_exact(4)) {
        *sample = f32::from_le_bytes(chunk.try_into().expect("four-byte sample"));
    }
}

fn check_status(status: i32, operation: &str) -> Result<()> {
    if status == STATUS_OK {
        Ok(())
    } else {
        bail!("portable plugin {operation} failed with status {status}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAIN: &str = r#"
        (module
          (memory (export "memory") 1)
          (global $gain (mut f32) (f32.const 1))
          (func (export "rackforge_abi_version") (result i32) i32.const 65537)
          (func (export "rackforge_input_ptr") (result i32) i32.const 0)
          (func (export "rackforge_output_ptr") (result i32) i32.const 1024)
          (func (export "rackforge_capacity_input_samples") (result i32) i32.const 256)
          (func (export "rackforge_capacity_output_samples") (result i32) i32.const 256)
          (func (export "rackforge_midi_ptr") (result i32) i32.const 4096)
          (func (export "rackforge_capacity_midi_events") (result i32) i32.const 64)
          (func (export "rackforge_parameter_ptr") (result i32) i32.const 5120)
          (func (export "rackforge_capacity_parameter_events") (result i32) i32.const 64)
          (func (export "rackforge_transfer_ptr") (result i32) i32.const 8192)
          (func (export "rackforge_capacity_transfer_bytes") (result i32) i32.const 1024)
          (func (export "rackforge_prepare") (param f64 i32 i32 i32) (result i32) i32.const 0)
          (func (export "rackforge_set_parameter") (param $index i32) (param $value f64) (result i32)
            local.get $value f32.demote_f64 global.set $gain i32.const 0)
          (func (export "rackforge_get_parameter") (param $index i32) (result f64)
            global.get $gain f64.promote_f32)
          (func (export "rackforge_reset") (result i32) i32.const 0)
          (func (export "rackforge_resource_begin") (param i32 i64) (result i32) i32.const -3)
          (func (export "rackforge_resource_write") (param i64 i32) (result i32) i32.const -3)
          (func (export "rackforge_resource_end") (result i32) i32.const -3)
          (func (export "rackforge_load_preset") (param i32) (result i32) i32.const -3)
          (func (export "rackforge_save_state") (result i32) i32.const -3)
          (func (export "rackforge_load_state") (param i32) (result i32) i32.const -3)
          (func (export "rackforge_process") (param $frames i32) (param $input_channels i32) (param $output_channels i32) (param $midi i32) (param $parameters i32) (result i32)
            (local $i i32) (local $count i32)
            local.get $parameters i32.const 0 i32.gt_s
            if
              i32.const 5128 f64.load f32.demote_f64 global.set $gain
            end
            local.get $frames local.get $output_channels i32.mul local.set $count
            (block $done
              (loop $copy
                local.get $i local.get $count i32.ge_u br_if $done
                local.get $i i32.const 4 i32.mul i32.const 1024 i32.add
                local.get $i i32.const 4 i32.mul f32.load global.get $gain f32.mul f32.store
                local.get $i i32.const 1 i32.add local.set $i
                br $copy))
            local.get $midi i32.const 0 i32.gt_s
            if
              i32.const 1024
              i32.const 4100 i32.load8_u f32.convert_i32_u f32.store
            end
            i32.const 0))
    "#;

    #[test]
    fn runs_one_portable_gain_module() {
        let bytes = wat::parse_str(GAIN).unwrap();
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine.compile(&bytes).unwrap();
        let mut instance = module.instantiate().unwrap();
        instance.prepare(48_000.0, 64, 2, 2).unwrap();
        instance.set_parameter(0, 0.5).unwrap();
        let input = [1.0, -1.0, 0.25, -0.25];
        let mut output = [0.0; 4];
        instance
            .process_interleaved(&input, &mut output, 2)
            .unwrap();
        assert_eq!(output, [0.5, -0.5, 0.125, -0.125]);
    }

    #[test]
    fn rejects_ambient_host_imports() {
        let bytes =
            wat::parse_str(r#"(module (import "wasi_snapshot_preview1" "fd_write" (func)))"#)
                .unwrap();
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let error = match engine.compile(&bytes) {
            Ok(_) => panic!("ambient import was accepted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("may not import"));
    }

    #[test]
    fn delivers_sample_positioned_midi_without_host_imports() {
        let bytes = wat::parse_str(GAIN).unwrap();
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine.compile(&bytes).unwrap();
        let mut instance = module.instantiate().unwrap();
        instance.prepare(48_000.0, 64, 2, 2).unwrap();
        let input = [0.0; 4];
        let mut output = [0.0; 4];
        let note_on = MidiEvent::new(1, &[0x90, 60, 100]).unwrap();
        instance
            .process_interleaved_with_midi(&input, &mut output, 2, &[note_on])
            .unwrap();
        assert_eq!(output[0], 0x90 as f32);
    }

    #[test]
    fn delivers_parameter_events_without_host_imports() {
        let bytes = wat::parse_str(GAIN).unwrap();
        let engine = PortableEngine::new(RuntimeLimits::default()).unwrap();
        let module = engine.compile(&bytes).unwrap();
        let mut instance = module.instantiate().unwrap();
        instance.prepare(48_000.0, 64, 2, 2).unwrap();
        let input = [1.0; 4];
        let mut output = [0.0; 4];
        let event = ParameterEvent {
            frame: 0,
            index: 0,
            value: 0.25,
        };
        instance
            .process_interleaved_with_events(&input, &mut output, 2, &[], &[event])
            .unwrap();
        assert_eq!(output, [0.25; 4]);
    }
}
