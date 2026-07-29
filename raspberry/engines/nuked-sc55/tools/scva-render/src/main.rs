use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavSpec, WavWriter};
use libloading::{Library, Symbol};
use std::env;
use std::path::{Path, PathBuf};

const SAMPLE_RATE: u32 = 44_100;
const BLOCK_SIZE: usize = 512;
const NOTE: u32 = 60;
const VELOCITY: u32 = 100;

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

fn render(dll_path: &Path, output_path: &Path) -> Result<()> {
    let dll_path = absolute(dll_path)?;
    let output_path = absolute(output_path)?;
    let dll_directory = dll_path
        .parent()
        .context("SCCore.dll has no parent directory")?;
    let old_directory = env::current_dir()?;
    env::set_current_dir(dll_directory)
        .with_context(|| format!("changing directory to {}", dll_directory.display()))?;

    let result = render_inner(&dll_path, &output_path);
    env::set_current_dir(&old_directory)
        .with_context(|| format!("restoring directory to {}", old_directory.display()))?;
    result
}

fn render_inner(dll_path: &Path, output_path: &Path) -> Result<()> {
    // SCCore is loaded only in this disposable command-line probe. All ABI signatures below
    // were independently documented by kode54's SCCore host and are checked by symbol name.
    let library = unsafe { Library::new(dll_path) }
        .with_context(|| format!("loading {}", dll_path.display()))?;

    unsafe {
        let initialize: Symbol<Initialize> = library.get(b"TG_initialize\0")?;
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
        set_max_block_size(BLOCK_SIZE as u32);
        set_interrupt_thread();

        // Program 0 (Piano 1), middle C, then note-off after one second.
        short_midi_in(0xC0, 0);
        short_midi_in(midi3(0x90, NOTE, VELOCITY), 0);

        let total_frames = SAMPLE_RATE as usize * 4;
        let note_off_frame = SAMPLE_RATE as usize;
        let mut rendered = 0;
        let mut left = Vec::with_capacity(total_frames);
        let mut right = Vec::with_capacity(total_frames);

        while rendered < total_frames {
            if rendered == note_off_frame {
                short_midi_in(midi3(0x80, NOTE, 0), 0);
            }
            let count = BLOCK_SIZE.min(total_frames - rendered);
            let mut block_left = vec![0.0_f32; count];
            let mut block_right = vec![0.0_f32; count];
            process(
                block_left.as_mut_ptr(),
                block_right.as_mut_ptr(),
                count as u32,
            );
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
        println!(
            "RENDERED {} frames={} peak={peak:.8} rms={rms:.8} voices_after_tail={voices}",
            output_path.display(),
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
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        bail!("usage: scva-render SCCore.dll OUTPUT.wav");
    }
    render(Path::new(&args[0]), Path::new(&args[1]))
}
