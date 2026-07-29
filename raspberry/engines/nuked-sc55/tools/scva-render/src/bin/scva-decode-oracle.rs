use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavSpec, WavWriter};
use libloading::{Library, Symbol};
use std::env;
use std::fs;
use std::mem;
use std::path::Path;

const SEGMENT_SIZE: usize = 1024 * 1024;
const TG_INITIALIZE_RVA: usize = 0x8e160;
const DECODER_INITIALIZE_RVA: usize = 0x43c40;
const DECODER_RENDER_RVA: usize = 0x43dc0;

type Initialize = unsafe extern "C" fn(i32) -> i32;
type DecoderInitialize =
    unsafe extern "system" fn(*mut DecoderState, *const u8, u32, u32, *const u8, u32);
type DecoderRender =
    unsafe extern "system" fn(*mut DecoderState, *mut f32, *const u32, *const f32, *const f32);

#[repr(C, align(16))]
struct DecoderState([u8; 0x50]);

fn parse_number(value: &str) -> Result<usize> {
    if let Some(hex) = value.strip_prefix("0x") {
        usize::from_str_radix(hex, 16).with_context(|| format!("invalid number {value:?}"))
    } else {
        value
            .parse()
            .with_context(|| format!("invalid number {value:?}"))
    }
}

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn run() -> Result<()> {
    let arguments: Vec<_> = env::args().skip(1).collect();
    let (required, render_request) = match arguments.as_slice() {
        [dll, wave, segment, start, loop_start, end] => {
            ([dll, wave, segment, start, loop_start, end], None)
        }
        [dll, wave, segment, start, loop_start, end, frames, output] => (
            [dll, wave, segment, start, loop_start, end],
            Some((parse_number(frames)?, output.as_str())),
        ),
        _ => bail!(
            "usage: scva-decode-oracle SCCore.dll WAVE_BANK SEGMENT START LOOP_START END \
             [FRAMES OUTPUT.wav]"
        ),
    };
    let [dll_path, wave_path, segment, start, loop_start, end] = required;
    let segment = parse_number(segment)?;
    let start = parse_number(start)?;
    let loop_start = parse_number(loop_start)?;
    let end = parse_number(end)?;
    if !(start <= loop_start && loop_start <= end && end < SEGMENT_SIZE) {
        bail!("invalid sample range");
    }

    let wave_bank = fs::read(wave_path).with_context(|| format!("reading {}", wave_path))?;
    let segment_count = wave_bank.len() / SEGMENT_SIZE;
    if wave_bank.len() % SEGMENT_SIZE != 0 || segment >= segment_count {
        bail!(
            "{} is not a segmented Wave ROM bank containing segment {segment}",
            wave_path
        );
    }
    let segment_bytes = &wave_bank[segment * SEGMENT_SIZE..(segment + 1) * SEGMENT_SIZE];
    let aligned_start = start & !0x1f;
    let data_start = aligned_start
        .checked_sub(0x20)
        .context("SCCore data-base bias underflow")?;
    let scale_start = (aligned_start >> 5)
        .checked_sub(0x20)
        .context("SCCore scale-base bias underflow")?;
    let data = &segment_bytes[data_start..];
    let scales = &segment_bytes[scale_start..];

    let library = unsafe { Library::new(Path::new(dll_path)) }
        .with_context(|| format!("loading {}", dll_path))?;
    let initialize: Symbol<Initialize> = unsafe { library.get(b"TG_initialize\0")? };
    let image_base = *initialize as usize - TG_INITIALIZE_RVA;
    let decoder: DecoderInitialize = unsafe { mem::transmute(image_base + DECODER_INITIALIZE_RVA) };
    let render: DecoderRender = unsafe { mem::transmute(image_base + DECODER_RENDER_RVA) };
    let mut state = DecoderState([0_u8; 0x50]);
    state.0[0x48] = 2;

    unsafe {
        decoder(
            &mut state,
            data.as_ptr(),
            (loop_start - aligned_start) as u32,
            (end - aligned_start) as u32,
            scales.as_ptr(),
            0,
        );
    }

    let mut accumulator = 0_i32;
    println!(
        "DECODER_ORACLE image_base=0x{image_base:x} segment={segment} \
         start=0x{start:x} aligned=0x{aligned_start:x} \
         data_base=0x{data_start:x} scale_base=0x{scale_start:x}"
    );
    for sample in 0..4 {
        let position = sample;
        let address = data_start + position;
        let delta = i32::from(segment_bytes[address] as i8);
        let scale_byte = segment_bytes[scale_start + (position >> 5)];
        let exponent = if position & 0x10 == 0 {
            scale_byte & 0x0f
        } else {
            scale_byte >> 4
        };
        accumulator = accumulator.wrapping_add(delta.wrapping_shl(u32::from(exponent)));
        println!(
            "SAMPLE index={sample} address=0x{address:x} delta={delta} exponent={exponent} \
             rust_accumulator={accumulator} native_float={:.9}",
            read_f32(&state.0, sample * 4)
        );
    }
    println!(
        "STATE position={} accumulator={} scale_nibble={} flags=0x{:02x}",
        read_i32(&state.0, 0x28),
        read_i32(&state.0, 0x40),
        state.0[0x49],
        state.0[0x48]
    );

    if let Some((frame_count, output)) = render_request {
        let increments = [0x1_0000_u32; 4];
        let inner_gains = [1.0_f32; 16];
        let outer_gains = [1.0_f32; 4];
        let mut rendered = Vec::with_capacity(frame_count);
        while rendered.len() < frame_count {
            let mut lanes = [0.0_f32; 128];
            unsafe {
                render(
                    &mut state,
                    lanes.as_mut_ptr(),
                    increments.as_ptr(),
                    inner_gains.as_ptr(),
                    outer_gains.as_ptr(),
                );
            }
            rendered.extend(lanes.iter().step_by(4).copied());
        }
        rendered.truncate(frame_count);

        let specification = WavSpec {
            channels: 1,
            sample_rate: 32_000,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(output, specification)
            .with_context(|| format!("creating {output}"))?;
        for sample in &rendered {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;
        let peak = rendered
            .iter()
            .fold(0.0_f32, |maximum, sample| maximum.max(sample.abs()));
        println!(
            "NATIVE_DECODE_RENDERED frames={} peak={peak:.9} output={output}",
            rendered.len()
        );
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}
