use anyhow::{Context, Result, bail};
use hound::{SampleFormat, WavSpec, WavWriter};
use rackforge_dls::{DlsBank, voices_for_note};
use std::env;
use std::path::Path;

const OUTPUT_RATE: u32 = 48_000;

fn inspect(path: &Path) -> Result<()> {
    let bank = DlsBank::open(path)?;
    println!(
        "DLS_OK file={} instruments={} waves={}",
        path.display(),
        bank.instruments.len(),
        bank.waves.len()
    );
    for instrument in &bank.instruments {
        println!(
            "INSTRUMENT bank=0x{:08x} program={} drum={} regions={} name={:?}",
            instrument.bank,
            instrument.program,
            instrument.is_drum(),
            instrument.regions.len(),
            instrument.name
        );
    }
    Ok(())
}

fn render(path: &Path, bank_number: u32, program: u32, note: u8, output: &Path) -> Result<()> {
    let bank = DlsBank::open(path)?;
    let instrument = bank
        .instrument(bank_number, program)
        .with_context(|| format!("DLS has no bank 0x{bank_number:08x}, program {program}"))?;
    let mut voices = voices_for_note(&bank, instrument, note, 100, OUTPUT_RATE)?;
    if voices.is_empty() {
        bail!(
            "instrument {:?} has no region for note {note}",
            instrument.name
        );
    }
    let specification = WavSpec {
        channels: 1,
        sample_rate: OUTPUT_RATE,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(output, specification)
        .with_context(|| format!("creating {}", output.display()))?;
    let mut peak = 0.0_f32;
    for frame in 0..OUTPUT_RATE as usize * 4 {
        if frame == OUTPUT_RATE as usize * 2 {
            for voice in &mut voices {
                voice.note_off();
            }
        }
        let sample = voices
            .iter_mut()
            .map(|voice| voice.next_sample())
            .sum::<f32>()
            * 0.8;
        peak = peak.max(sample.abs());
        writer.write_sample(sample.clamp(-1.0, 1.0))?;
    }
    writer.finalize()?;
    println!(
        "RENDERED instrument={:?} bank=0x{bank_number:08x} program={program} note={note} \
         voices={} peak={peak:.6} output={}",
        instrument.name,
        voices.len(),
        output.display()
    );
    Ok(())
}

fn parse_u32(value: &str) -> Result<u32> {
    if let Some(hex) = value.strip_prefix("0x") {
        Ok(u32::from_str_radix(hex, 16)?)
    } else {
        Ok(value.parse()?)
    }
}

fn run() -> Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice() {
        [command, input] if command == "inspect" => inspect(Path::new(input)),
        [command, input, bank, program, note, output] if command == "render" => {
            let note = note.parse::<u8>().context("NOTE must be in 0..=127")?;
            render(
                Path::new(input),
                parse_u32(bank)?,
                parse_u32(program)?,
                note,
                Path::new(output),
            )
        }
        _ => bail!(
            "usage:\n  rackforge-dls inspect BANK.dls\n  \
             rackforge-dls render BANK.dls BANK PROGRAM NOTE OUTPUT.wav"
        ),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}
