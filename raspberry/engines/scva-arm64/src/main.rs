use anyhow::{Context, Result, bail};
use artupy_scva_bank::{GroupId, WaveBankSet};
use hound::{SampleFormat, WavSpec, WavWriter};
use std::env;
use std::path::Path;
use std::str::FromStr;

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
    output: &Path,
) -> Result<()> {
    let banks = WaveBankSet::open(directory)?;
    let segment = banks.group(group_id).segment(segment_index)?;
    let decoded = segment.decode_fce_dpcm(start, length, 0)?;
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
        "DECODED group={group_id} segment={segment_index} start=0x{start:x} \
         length=0x{length:x} peak={peak} output={}",
        output.display()
    );
    Ok(())
}

fn usage() -> &'static str {
    "usage:\n  artupy-scva-bank inspect BANK_DIRECTORY\n  \
     artupy-scva-bank decode BANK_DIRECTORY GROUP SEGMENT START LENGTH OUTPUT.wav\n\
     numbers may be decimal or 0x-prefixed hexadecimal"
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, directory] if command == "inspect" => inspect(Path::new(directory)),
        [command, directory, group, segment, start, length, output] if command == "decode" => {
            decode(
                Path::new(directory),
                GroupId::from_str(group).map_err(anyhow::Error::msg)?,
                parse_number(segment)?,
                parse_number(start)?,
                parse_number(length)?,
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
