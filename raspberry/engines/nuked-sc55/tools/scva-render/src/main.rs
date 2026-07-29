use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavSpec, WavWriter};
use libloading::{Library, Symbol};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

const SAMPLE_RATE: u32 = 44_100;
const BLOCK_SIZE: usize = 512;
const NOTE: u32 = 60;
const VELOCITY: u32 = 100;
const TG_INITIALIZE_RVA: usize = 0x8e160;

#[derive(Debug)]
struct ProbeConfig {
    program: u32,
    note: u32,
    velocity: u32,
    patch_u32: Option<(usize, u32)>,
    patch_u8: Option<(usize, u8)>,
    dump_rva: Option<(usize, usize, PathBuf)>,
    dump_pointer_rva: Option<(usize, usize, PathBuf)>,
    dump_block: usize,
    block_size: usize,
    trace_rva: Option<(usize, usize, PathBuf)>,
    duration_ms: u32,
    note_off_ms: u32,
    controls: Vec<(u32, u32)>,
}

struct TemporaryDll {
    path: PathBuf,
}

impl Drop for TemporaryDll {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path) {
            eprintln!(
                "WARNING: could not remove temporary probe DLL {}: {error}",
                self.path.display()
            );
        }
    }
}

type Initialize = unsafe extern "C" fn(i32) -> i32;
type Activate = unsafe extern "C" fn(f32, i32);
type Deactivate = unsafe extern "C" fn();
type SetSampleRate = unsafe extern "C" fn(f32);
type SetMaxBlockSize = unsafe extern "C" fn(u32);
type SetInterruptThread = unsafe extern "C" fn();
type ShortMidiIn = unsafe extern "C" fn(u32, u32);
type Process = unsafe extern "C" fn(*mut f32, *mut f32, u32);
type RunningVoices = unsafe extern "C" fn() -> u32;

fn midi3(status: u32, data1: u32, data2: u32) -> u32 {
    status | (data1 << 8) | (data2 << 16)
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn parse_number(value: &str) -> Result<u32> {
    let parsed = if let Some(hex) = value.strip_prefix("0x") {
        u32::from_str_radix(hex, 16)
    } else {
        value.parse()
    };
    parsed.with_context(|| format!("invalid number {value:?}"))
}

fn make_patched_dll(source: &Path, offset: usize, value: u32) -> Result<TemporaryDll> {
    let mut bytes = fs::read(source).with_context(|| format!("reading {}", source.display()))?;
    let end = offset.checked_add(4).context("patch offset overflow")?;
    let file_size = bytes.len();
    let target = bytes.get_mut(offset..end).with_context(|| {
        format!(
            "patch 0x{offset:x}..0x{end:x} is outside {} (size 0x{file_size:x})",
            source.display()
        )
    })?;
    let old_value = u32::from_le_bytes(target.try_into().unwrap());
    target.copy_from_slice(&value.to_le_bytes());

    let file_name = format!(".rackforge-sccore-probe-{}.dll", std::process::id());
    let path = source
        .parent()
        .context("SCCore.dll has no parent directory")?
        .join(file_name);
    if path.exists() {
        bail!("temporary probe path already exists: {}", path.display());
    }
    fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "PATCHED_COPY {} file_offset=0x{offset:x} old=0x{old_value:08x} new=0x{value:08x}",
        path.display()
    );
    Ok(TemporaryDll { path })
}

fn make_patched_dll_u8(source: &Path, offset: usize, value: u8) -> Result<TemporaryDll> {
    let mut bytes = fs::read(source).with_context(|| format!("reading {}", source.display()))?;
    let old_value = *bytes.get(offset).with_context(|| {
        format!(
            "patch offset 0x{offset:x} is outside {} (size 0x{:x})",
            source.display(),
            bytes.len()
        )
    })?;
    bytes[offset] = value;

    let file_name = format!(".rackforge-sccore-probe-{}.dll", std::process::id());
    let path = source
        .parent()
        .context("SCCore.dll has no parent directory")?
        .join(file_name);
    if path.exists() {
        bail!("temporary probe path already exists: {}", path.display());
    }
    fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    println!(
        "PATCHED_COPY {} file_offset=0x{offset:x} old=0x{old_value:02x} new=0x{value:02x}",
        path.display()
    );
    Ok(TemporaryDll { path })
}

fn write_wav(path: &Path, left: &[f32], right: &[f32]) -> Result<()> {
    let spec = WavSpec {
        channels: 2,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer =
        WavWriter::create(path, spec).with_context(|| format!("creating {}", path.display()))?;
    for (&left_sample, &right_sample) in left.iter().zip(right) {
        writer.write_sample(left_sample)?;
        writer.write_sample(right_sample)?;
    }
    writer.finalize()?;
    Ok(())
}

fn write_mono_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer =
        WavWriter::create(path, spec).with_context(|| format!("creating {}", path.display()))?;
    for sample in samples {
        writer.write_sample(*sample)?;
    }
    writer.finalize()?;
    Ok(())
}

fn render(dll_path: &Path, output_path: &Path, config: &ProbeConfig) -> Result<()> {
    let dll_path = absolute(dll_path)?;
    let output_path = absolute(output_path)?;
    let patched = match (config.patch_u32, config.patch_u8) {
        (Some((offset, value)), None) => Some(make_patched_dll(&dll_path, offset, value)?),
        (None, Some((offset, value))) => Some(make_patched_dll_u8(&dll_path, offset, value)?),
        (None, None) => None,
        (Some(_), Some(_)) => bail!("only one binary patch may be applied per probe"),
    };
    let load_path = patched
        .as_ref()
        .map_or(dll_path.as_path(), |temporary| temporary.path.as_path());
    let dll_directory = dll_path
        .parent()
        .context("SCCore.dll has no parent directory")?;
    let old_directory = env::current_dir()?;
    env::set_current_dir(dll_directory)
        .with_context(|| format!("changing directory to {}", dll_directory.display()))?;

    let result = render_inner(load_path, &output_path, config);
    env::set_current_dir(&old_directory)
        .with_context(|| format!("restoring directory to {}", old_directory.display()))?;
    drop(patched);
    result
}

fn render_inner(dll_path: &Path, output_path: &Path, config: &ProbeConfig) -> Result<()> {
    // SCCore is loaded only in this disposable command-line probe. All ABI signatures below
    // were independently documented by kode54's SCCore host and are checked by symbol name.
    let library = unsafe { Library::new(dll_path) }
        .with_context(|| format!("loading {}", dll_path.display()))?;

    unsafe {
        let initialize: Symbol<Initialize> = library.get(b"TG_initialize\0")?;
        let image_base = *initialize as usize - TG_INITIALIZE_RVA;
        println!("SCCORE_IMAGE_BASE 0x{image_base:x}");
        let activate: Symbol<Activate> = library.get(b"TG_activate\0")?;
        let deactivate: Symbol<Deactivate> = library.get(b"TG_deactivate\0")?;
        let set_sample_rate: Symbol<SetSampleRate> = library.get(b"TG_setSampleRate\0")?;
        let set_max_block_size: Symbol<SetMaxBlockSize> = library.get(b"TG_setMaxBlockSize\0")?;
        let set_interrupt_thread: Symbol<SetInterruptThread> =
            library.get(b"TG_setInterruptThreadIdAtThisTime\0")?;
        let short_midi_in: Symbol<ShortMidiIn> = library.get(b"TG_ShortMidiIn\0")?;
        let process: Symbol<Process> = library.get(b"TG_Process\0")?;
        let running_voices: Symbol<RunningVoices> =
            library.get(b"TG_XPgetCurTotalRunningVoices\0")?;

        let status = initialize(0);
        if status < 0 {
            bail!("TG_initialize failed with status {status}");
        }

        activate(SAMPLE_RATE as f32, 1024);
        set_max_block_size(256);
        set_sample_rate(SAMPLE_RATE as f32);
        set_sample_rate(SAMPLE_RATE as f32);
        // SCCore rejects a configured maximum below 64 even though TG_Process
        // itself accepts 32-frame chunks (the internal SIMD quantum).
        set_max_block_size(config.block_size.max(64) as u32);
        set_interrupt_thread();

        short_midi_in(midi3(0xC0, config.program, 0), 0);
        for &(control, value) in &config.controls {
            short_midi_in(midi3(0xB0, control, value), 0);
        }
        short_midi_in(midi3(0x90, config.note, config.velocity), 0);

        let total_frames = SAMPLE_RATE as usize * config.duration_ms as usize / 1000;
        let note_off_frame = SAMPLE_RATE as usize * config.note_off_ms as usize / 1000;
        let mut rendered = 0;
        let mut note_off_sent = false;
        let mut left = Vec::with_capacity(total_frames);
        let mut right = Vec::with_capacity(total_frames);
        let mut traced = Vec::new();

        while rendered < total_frames {
            if !note_off_sent && rendered == note_off_frame {
                short_midi_in(midi3(0x80, config.note, 0), 0);
                note_off_sent = true;
            }
            let mut count = config.block_size.min(total_frames - rendered);
            if !note_off_sent && rendered < note_off_frame {
                count = count.min(note_off_frame - rendered);
            }
            let mut block_left = vec![0.0_f32; count];
            let mut block_right = vec![0.0_f32; count];
            process(
                block_left.as_mut_ptr(),
                block_right.as_mut_ptr(),
                count as u32,
            );
            let block_index = rendered / config.block_size;
            if block_index == config.dump_block
                && let Some((rva, length, path)) = &config.dump_rva
            {
                let bytes = std::slice::from_raw_parts((image_base + rva) as *const u8, *length);
                fs::write(path, bytes)
                    .with_context(|| format!("writing memory dump {}", path.display()))?;
                println!(
                    "MEMORY_DUMP rva=0x{rva:x} length=0x{length:x} output={}",
                    path.display()
                );
            }
            if block_index == config.dump_block
                && let Some((pointer_rva, length, path)) = &config.dump_pointer_rva
            {
                let pointer = *((image_base + pointer_rva) as *const usize);
                let bytes = std::slice::from_raw_parts(pointer as *const u8, *length);
                fs::write(path, bytes)
                    .with_context(|| format!("writing pointer memory dump {}", path.display()))?;
                println!(
                    "POINTER_MEMORY_DUMP pointer_rva=0x{pointer_rva:x} \
                     address=0x{pointer:x} length=0x{length:x} output={}",
                    path.display()
                );
            }
            if let Some((rva, float_count, _)) = &config.trace_rva {
                let values =
                    std::slice::from_raw_parts((image_base + rva) as *const f32, *float_count);
                traced.extend_from_slice(values);
            }
            left.extend_from_slice(&block_left);
            right.extend_from_slice(&block_right);
            rendered += count;
        }

        let voices = running_voices();
        deactivate();

        let peak = left
            .iter()
            .chain(&right)
            .fold(0.0_f32, |value, sample| value.max(sample.abs()));
        let sum_squares: f64 = left
            .iter()
            .chain(&right)
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum();
        let rms = (sum_squares / (left.len() + right.len()) as f64).sqrt();

        write_wav(output_path, &left, &right)?;
        if let Some((rva, float_count, path)) = &config.trace_rva {
            write_mono_wav(path, &traced)?;
            println!(
                "MEMORY_TRACE rva=0x{rva:x} floats_per_block={float_count} \
                 frames={} output={}",
                traced.len(),
                path.display()
            );
        }
        println!(
            "RENDERED {} program={} note={} velocity={} frames={} \
             peak={peak:.8} rms={rms:.8} voices_after_tail={voices}",
            output_path.display(),
            config.program,
            config.note,
            config.velocity,
            left.len()
        );
        if peak == 0.0 {
            bail!("SCCore rendered digital silence");
        }
    }

    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let mut config = ProbeConfig {
        program: 0,
        note: NOTE,
        velocity: VELOCITY,
        patch_u32: None,
        patch_u8: None,
        dump_rva: None,
        dump_pointer_rva: None,
        dump_block: 0,
        block_size: BLOCK_SIZE,
        trace_rva: None,
        duration_ms: 4_000,
        note_off_ms: 1_000,
        controls: Vec::new(),
    };
    let mut positional: Vec<OsString> = Vec::new();

    while let Some(argument) = args.next() {
        match argument.to_string_lossy().as_ref() {
            "--program" => {
                let value = args.next().context("--program requires a value")?;
                config.program = parse_number(&value.to_string_lossy())?;
            }
            "--note" => {
                let value = args.next().context("--note requires a value")?;
                config.note = parse_number(&value.to_string_lossy())?;
            }
            "--velocity" => {
                let value = args.next().context("--velocity requires a value")?;
                config.velocity = parse_number(&value.to_string_lossy())?;
            }
            "--patch-u32" => {
                let offset = args.next().context("--patch-u32 requires OFFSET VALUE")?;
                let value = args.next().context("--patch-u32 requires OFFSET VALUE")?;
                config.patch_u32 = Some((
                    parse_number(&offset.to_string_lossy())? as usize,
                    parse_number(&value.to_string_lossy())?,
                ));
            }
            "--patch-u8" => {
                let offset = args.next().context("--patch-u8 requires OFFSET VALUE")?;
                let value = args.next().context("--patch-u8 requires OFFSET VALUE")?;
                let value = parse_number(&value.to_string_lossy())?;
                if value > u8::MAX.into() {
                    bail!("--patch-u8 VALUE must fit in one byte");
                }
                config.patch_u8 = Some((
                    parse_number(&offset.to_string_lossy())? as usize,
                    value as u8,
                ));
            }
            "--dump-rva" => {
                let rva = args
                    .next()
                    .context("--dump-rva requires RVA LENGTH OUTPUT")?;
                let length = args
                    .next()
                    .context("--dump-rva requires RVA LENGTH OUTPUT")?;
                let output = args
                    .next()
                    .context("--dump-rva requires RVA LENGTH OUTPUT")?;
                config.dump_rva = Some((
                    parse_number(&rva.to_string_lossy())? as usize,
                    parse_number(&length.to_string_lossy())? as usize,
                    PathBuf::from(output),
                ));
            }
            "--dump-pointer-rva" => {
                let rva = args
                    .next()
                    .context("--dump-pointer-rva requires POINTER_RVA LENGTH OUTPUT")?;
                let length = args
                    .next()
                    .context("--dump-pointer-rva requires POINTER_RVA LENGTH OUTPUT")?;
                let output = args
                    .next()
                    .context("--dump-pointer-rva requires POINTER_RVA LENGTH OUTPUT")?;
                config.dump_pointer_rva = Some((
                    parse_number(&rva.to_string_lossy())? as usize,
                    parse_number(&length.to_string_lossy())? as usize,
                    PathBuf::from(output),
                ));
            }
            "--block-size" => {
                let value = args.next().context("--block-size requires a value")?;
                config.block_size = parse_number(&value.to_string_lossy())? as usize;
                if !(32..=BLOCK_SIZE).contains(&config.block_size) {
                    bail!("--block-size must be in 32..={BLOCK_SIZE}");
                }
            }
            "--duration-ms" => {
                let value = args.next().context("--duration-ms requires a value")?;
                config.duration_ms = parse_number(&value.to_string_lossy())?;
            }
            "--note-off-ms" => {
                let value = args.next().context("--note-off-ms requires a value")?;
                config.note_off_ms = parse_number(&value.to_string_lossy())?;
            }
            "--cc" => {
                let control = args.next().context("--cc requires NUMBER VALUE")?;
                let value = args.next().context("--cc requires NUMBER VALUE")?;
                config.controls.push((
                    parse_number(&control.to_string_lossy())?,
                    parse_number(&value.to_string_lossy())?,
                ));
            }
            "--dump-block" => {
                let value = args.next().context("--dump-block requires an index")?;
                config.dump_block = parse_number(&value.to_string_lossy())? as usize;
            }
            "--trace-rva" => {
                let rva = args
                    .next()
                    .context("--trace-rva requires RVA FLOATS OUTPUT.wav")?;
                let count = args
                    .next()
                    .context("--trace-rva requires RVA FLOATS OUTPUT.wav")?;
                let output = args
                    .next()
                    .context("--trace-rva requires RVA FLOATS OUTPUT.wav")?;
                config.trace_rva = Some((
                    parse_number(&rva.to_string_lossy())? as usize,
                    parse_number(&count.to_string_lossy())? as usize,
                    PathBuf::from(output),
                ));
            }
            option if option.starts_with('-') => bail!("unknown option: {option}"),
            _ => positional.push(argument),
        }
    }

    if config.program > 127 || config.note > 127 || config.velocity > 127 {
        bail!("program, note, and velocity must be MIDI values in 0..=127");
    }
    if config
        .controls
        .iter()
        .any(|&(control, value)| control > 127 || value > 127)
    {
        bail!("CC numbers and values must be MIDI values in 0..=127");
    }
    if config.duration_ms == 0 || config.note_off_ms > config.duration_ms {
        bail!("duration must be positive and note-off must not exceed it");
    }
    if positional.len() != 2 {
        bail!(
            "usage: scva-render [--program N] [--note N] [--velocity N] \
             [--patch-u32 FILE_OFFSET VALUE] [--patch-u8 FILE_OFFSET VALUE] \
             [--dump-rva RVA LENGTH OUTPUT] \
             [--dump-pointer-rva POINTER_RVA LENGTH OUTPUT] \
             [--dump-block INDEX] [--block-size N] \
             [--duration-ms N] [--note-off-ms N] \
             [--cc NUMBER VALUE]... \
             [--trace-rva RVA FLOATS OUTPUT.wav] \
             SCCore.dll OUTPUT.wav"
        );
    }
    render(
        Path::new(&positional[0]),
        Path::new(&positional[1]),
        &config,
    )
}
