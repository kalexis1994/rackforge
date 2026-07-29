use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavSpec, WavWriter};
use rackforge_scva_bank::{ControlBankSet, GroupId, WaveBankSet};
use std::env;
use std::path::Path;
use std::str::FromStr;

const PREVIEW_RATE: u32 = 32_000;
const ROM_RATE: f64 = 44_100.0;

fn parse_number(value: &str) -> Result<usize> {
    if let Some(hex) = value.strip_prefix("0x") {
        usize::from_str_radix(hex, 16).with_context(|| format!("invalid hex number {value:?}"))
    } else {
        value
            .parse()
            .with_context(|| format!("invalid decimal number {value:?}"))
    }
}

fn inspect(directory: &Path) -> Result<()> {
    let banks = WaveBankSet::open(directory)?;
    println!(
        "BANK_SET directory={} profile={}",
        directory.display(),
        if banks.is_known_112() {
            "Sound Canvas VA 1.1.2 exact"
        } else {
            "structurally compatible, unknown hashes"
        }
    );
    for group in banks.groups() {
        println!(
            "GROUP id={} file={} segments={} sha256={} known_112={}",
            group.id(),
            group.file_name(),
            group.segment_count(),
            group.sha256(),
            group.is_known_112()
        );
        for index in 0..group.segment_count() {
            let header = group.header(index)?;
            println!(
                "  SEGMENT index={index} marker={:?} date={} config={:08x},{:08x},{:08x}",
                header.marker,
                header.date,
                header.config_words[0],
                header.config_words[1],
                header.config_words[2]
            );
        }
    }
    Ok(())
}

fn decode(
    directory: &Path,
    group_id: GroupId,
    segment_index: usize,
    start: usize,
    length: usize,
    sccore_window: bool,
    output: &Path,
) -> Result<()> {
    let banks = WaveBankSet::open(directory)?;
    let segment = banks.group(group_id).segment(segment_index)?;
    let decoded = if sccore_window {
        let end = start
            .checked_add(length.saturating_sub(1))
            .context("SCCore decode range overflow")?;
        segment.decode_sccore_window(start, end)?
    } else {
        segment.decode_fce_dpcm(start, length, 0)?
    };
    let peak = decoded
        .iter()
        .map(|sample| i64::from(*sample).abs())
        .max()
        .unwrap_or(0);
    if peak == 0 {
        bail!("decoded range is digital silence");
    }

    let scale = 0.95_f32 / peak as f32;
    let spec = WavSpec {
        channels: 1,
        sample_rate: 32_000,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(output, spec)
        .with_context(|| format!("creating {}", output.display()))?;
    for sample in decoded {
        writer.write_sample(sample as f32 * scale)?;
    }
    writer.finalize()?;
    println!(
        "DECODED mode={} group={group_id} segment={segment_index} start=0x{start:x} \
         length=0x{length:x} peak={peak} output={}",
        if sccore_window { "sccore" } else { "direct" },
        output.display()
    );
    Ok(())
}

fn resolve(control_directory: &Path, tone: usize, note: usize) -> Result<()> {
    let controls = ControlBankSet::open(control_directory)?;
    let resolution = controls.resolve(tone, note)?;
    println!(
        "TONE index={} name={:?} note={} partial_mask=0x{:02x}",
        resolution.tone, resolution.tone_name, resolution.note, resolution.partial_mask
    );
    for partial in resolution.partials {
        let descriptor = partial
            .descriptor
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join("");
        println!(
            "  partial={} map={} map_name={:?} key_bias={} mapped_note={} slot={} \
             upper_note={} map_value={} sample={} descriptor={descriptor} \
             group={} segment={} start=0x{:x} loop_start=0x{:x} end=0x{:x} \
             looped={} reverse={} selector=0x{:x} root_key={} \
             fine_tune={} root_pitch_milli={}",
            partial.partial,
            partial.wave_map,
            partial.wave_map_name,
            partial.key_bias,
            partial.mapped_note,
            partial.slot,
            partial.upper_note,
            partial.map_value,
            partial.sample,
            partial.sample_location.group,
            partial.sample_location.group_segment,
            partial.sample_location.start,
            partial.sample_location.loop_start,
            partial.sample_location.end,
            partial.sample_location.looped,
            partial.sample_location.reverse,
            partial.sample_location.wave_selector,
            partial.sample_location.root_key,
            partial.sample_location.fine_tune_word,
            partial.sample_location.root_pitch_milli
        );
    }
    Ok(())
}

fn render_tone(
    bank_directory: &Path,
    control_directory: &Path,
    tone: usize,
    note: usize,
    output: &Path,
) -> Result<()> {
    let banks = WaveBankSet::open(bank_directory)?;
    let controls = ControlBankSet::open(control_directory)?;
    let resolution = controls.resolve(tone, note)?;
    let mut rendered_partials: Vec<Vec<f64>> = Vec::new();

    for partial in &resolution.partials {
        let location = partial.sample_location;
        if location.reverse {
            bail!(
                "tone {tone} partial {} uses reverse playback, not supported by the preview renderer",
                partial.partial
            );
        }
        let decoded = banks
            .group(location.group)
            .segment(location.group_segment)?
            .decode_sccore_window(location.start, location.end)?;
        let target_pitch_milli = i32::from(partial.mapped_note) * 1_000;
        let increment = 2_f64
            .powf(f64::from(target_pitch_milli - location.root_pitch_milli) / 12_000.0)
            * ROM_RATE
            / f64::from(PREVIEW_RATE);
        let output_length = ((decoded.len() as f64 / increment).ceil() as usize)
            .min(PREVIEW_RATE as usize * 10)
            .max(1);
        let mut resampled = Vec::with_capacity(output_length);
        for output_index in 0..output_length {
            let position = output_index as f64 * increment;
            let index = position.floor() as usize;
            if index >= decoded.len() {
                break;
            }
            let fraction = position - index as f64;
            let first = f64::from(decoded[index]);
            let second = f64::from(*decoded.get(index + 1).unwrap_or(&decoded[index]));
            resampled.push(first + (second - first) * fraction);
        }
        println!(
            "PARTIAL_RENDER partial={} sample={} group={} segment={} source_frames={} \
             increment={increment:.8} output_frames={}",
            partial.partial,
            partial.sample,
            location.group,
            location.group_segment,
            decoded.len(),
            resampled.len()
        );
        rendered_partials.push(resampled);
    }

    let frame_count = rendered_partials
        .iter()
        .map(Vec::len)
        .max()
        .context("tone has no enabled partials")?;
    let mut mixed = vec![0_f64; frame_count];
    for partial in &rendered_partials {
        for (target, sample) in mixed.iter_mut().zip(partial) {
            *target += *sample;
        }
    }
    let peak = mixed
        .iter()
        .fold(0_f64, |maximum, sample| maximum.max(sample.abs()));
    if peak == 0.0 {
        bail!("rendered tone is digital silence");
    }
    let scale = 0.90 / peak;
    let spec = WavSpec {
        channels: 1,
        sample_rate: PREVIEW_RATE,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(output, spec)
        .with_context(|| format!("creating {}", output.display()))?;
    for sample in mixed {
        writer.write_sample((sample * scale) as f32)?;
    }
    writer.finalize()?;
    println!(
        "TONE_RENDERED tone={} name={:?} note={} partials={} frames={} peak={peak:.3} output={}",
        resolution.tone,
        resolution.tone_name,
        resolution.note,
        rendered_partials.len(),
        frame_count,
        output.display()
    );
    Ok(())
}

fn usage() -> &'static str {
    "usage:\n  rackforge-scva-bank inspect BANK_DIRECTORY\n  \
     rackforge-scva-bank resolve CONTROL_DIRECTORY TONE MIDI_NOTE\n  \
     rackforge-scva-bank render-tone BANK_DIRECTORY CONTROL_DIRECTORY TONE MIDI_NOTE OUTPUT.wav\n  \
     rackforge-scva-bank decode BANK_DIRECTORY GROUP SEGMENT START LENGTH OUTPUT.wav\n  \
     rackforge-scva-bank decode-sccore BANK_DIRECTORY GROUP SEGMENT START LENGTH OUTPUT.wav\n\
     numbers may be decimal or 0x-prefixed hexadecimal"
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, directory] if command == "inspect" => inspect(Path::new(directory)),
        [command, directory, tone, note] if command == "resolve" => resolve(
            Path::new(directory),
            parse_number(tone)?,
            parse_number(note)?,
        ),
        [command, banks, controls, tone, note, output] if command == "render-tone" => render_tone(
            Path::new(banks),
            Path::new(controls),
            parse_number(tone)?,
            parse_number(note)?,
            Path::new(output),
        ),
        [command, directory, group, segment, start, length, output]
            if command == "decode" || command == "decode-sccore" =>
        {
            decode(
                Path::new(directory),
                GroupId::from_str(group).map_err(anyhow::Error::msg)?,
                parse_number(segment)?,
                parse_number(start)?,
                parse_number(length)?,
                command == "decode-sccore",
                Path::new(output),
            )
        }
        _ => bail!(usage()),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}
