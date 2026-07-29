#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("rackforge-dls-live is only available on Linux.");
}

#[cfg(target_os = "linux")]
mod linux {
    use alsa::pcm::{Access, Format, HwParams, PCM};
    use alsa::{Direction, ValueOr};
    use anyhow::{Context, Result, anyhow, bail};
    use midir::{Ignore, MidiInput};
    use rackforge_dls::{DlsBank, Voice, voices_for_note};
    use std::collections::VecDeque;
    use std::env;
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant};

    const SAMPLE_RATE: u32 = 48_000;
    const CHANNELS: u32 = 2;
    const PERIOD_FRAMES: usize = 128;
    const BUFFER_FRAMES: u32 = 384;
    const MAX_VOICES: usize = 32;
    const DEFAULT_GAIN: f32 = 0.65;

    #[derive(Debug)]
    enum MidiEvent {
        NoteOn { note: u8, velocity: u8 },
        NoteOff { note: u8 },
    }

    #[derive(Debug)]
    struct Options {
        bank_path: PathBuf,
        bank: u32,
        program: u32,
        gain: f32,
    }

    pub fn run() -> Result<()> {
        let options = parse_options()?;
        let bank = DlsBank::open(&options.bank_path)
            .with_context(|| format!("could not load {}", options.bank_path.display()))?;
        let instrument = bank
            .instrument(options.bank, options.program)
            .ok_or_else(|| {
                anyhow!(
                    "DLS instrument not found: bank={} program={}",
                    options.bank,
                    options.program
                )
            })?;

        eprintln!(
            "DLS_BANK path={} instruments={} waves={}",
            options.bank_path.display(),
            bank.instruments.len(),
            bank.waves.len()
        );
        eprintln!(
            "DLS_PROGRAM bank={} program={} name={} regions={}",
            options.bank,
            options.program,
            instrument.name,
            instrument.regions.len()
        );

        let (receiver, _connection) = open_midi_input()?;
        let pcm = open_pcm()?;
        let io = pcm.io_i32().context("could not create ALSA i32 writer")?;
        pcm.prepare().context("could not prepare ALSA device")?;

        let mut voices: Vec<Voice> = Vec::with_capacity(MAX_VOICES);
        let mut pending = VecDeque::new();
        let mut output = vec![0_i32; PERIOD_FRAMES * CHANNELS as usize];
        let mut meter_started = Instant::now();
        let mut meter_peak = 0.0_f32;

        eprintln!(
            "READY_TO_PLAY engine=dls rate={} period={} buffer={} gain={:.3}",
            SAMPLE_RATE, PERIOD_FRAMES, BUFFER_FRAMES, options.gain
        );

        loop {
            while let Ok(event) = receiver.try_recv() {
                pending.push_back(event);
            }

            while let Some(event) = pending.pop_front() {
                match event {
                    MidiEvent::NoteOn { note, velocity } => {
                        voices.retain(|voice| voice.note != note);
                        let new_voices =
                            voices_for_note(&bank, instrument, note, velocity, SAMPLE_RATE)
                                .with_context(|| {
                                    format!(
                                        "could not create voice for note={note} velocity={velocity}"
                                    )
                                })?;
                        if new_voices.is_empty() {
                            eprintln!("NOTE_UNMAPPED note={note} velocity={velocity}");
                            continue;
                        }
                        for voice in new_voices {
                            if voices.len() >= MAX_VOICES {
                                voices.remove(0);
                            }
                            voices.push(voice);
                        }
                    }
                    MidiEvent::NoteOff { note } => {
                        for voice in voices.iter_mut().filter(|voice| voice.note == note) {
                            voice.note_off();
                        }
                    }
                }
            }

            for frame in 0..PERIOD_FRAMES {
                let mut mixed = 0.0_f32;
                for voice in &mut voices {
                    mixed += voice.next_sample();
                }
                voices.retain(|voice| !voice.is_finished());

                let sample = (mixed * options.gain).clamp(-1.0, 1.0);
                meter_peak = meter_peak.max(sample.abs());
                let encoded = (sample * i32::MAX as f32) as i32;
                let offset = frame * CHANNELS as usize;
                output[offset] = encoded;
                output[offset + 1] = encoded;
            }

            match io.writei(&output) {
                Ok(_) => {}
                Err(error) => {
                    if error.errno() == libc::EPIPE {
                        eprintln!("ALSA_XRUN recovering");
                        pcm.prepare()
                            .context("could not recover from ALSA underrun")?;
                    } else {
                        return Err(error).context("ALSA write failed");
                    }
                }
            }

            if meter_started.elapsed() >= Duration::from_secs(2) {
                eprintln!("AUDIO_METER peak={meter_peak:.4} voices={}", voices.len());
                meter_peak = 0.0;
                meter_started = Instant::now();
            }
        }
    }

    fn parse_options() -> Result<Options> {
        let mut args = env::args().skip(1);
        let mut bank = 0_u32;
        let mut program = 0_u32;
        let mut gain = DEFAULT_GAIN;
        let mut bank_path = None;

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--bank" => {
                    let value = args.next().context("--bank requires a value")?;
                    bank = parse_u32(&value).context("invalid --bank value")?;
                }
                "--program" => {
                    let value = args.next().context("--program requires a value")?;
                    program = parse_u32(&value).context("invalid --program value")?;
                }
                "--gain" => {
                    let value = args.next().context("--gain requires a value")?;
                    gain = value.parse::<f32>().context("invalid --gain value")?;
                    if !(0.0..=1.0).contains(&gain) {
                        bail!("--gain must be between 0 and 1");
                    }
                }
                "-h" | "--help" => {
                    print_usage();
                    std::process::exit(0);
                }
                value if value.starts_with('-') => bail!("unknown option: {value}"),
                value => {
                    if bank_path.replace(PathBuf::from(value)).is_some() {
                        bail!("only one DLS bank path may be supplied");
                    }
                }
            }
        }

        let bank_path = bank_path.context("missing DLS bank path")?;
        Ok(Options {
            bank_path,
            bank,
            program,
            gain,
        })
    }

    fn parse_u32(value: &str) -> Result<u32> {
        if let Some(hexadecimal) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            Ok(u32::from_str_radix(hexadecimal, 16)?)
        } else {
            Ok(value.parse()?)
        }
    }

    fn print_usage() {
        eprintln!("Usage: rackforge-dls-live [--bank N] [--program N] [--gain 0..1] BANK.dls");
    }

    fn open_midi_input() -> Result<(Receiver<MidiEvent>, midir::MidiInputConnection<()>)> {
        let mut midi_input = MidiInput::new("RackForge DLS MIDI input")
            .context("could not initialize MIDI input")?;
        midi_input.ignore(Ignore::None);

        let ports = midi_input.ports();
        let mut candidates = Vec::new();
        for port in &ports {
            let name = midi_input
                .port_name(port)
                .unwrap_or_else(|_| "(unknown MIDI port)".to_string());
            if is_keylab_midi_port(&name) {
                candidates.push((port.clone(), name));
            }
        }

        let (port, name) = candidates.into_iter().next().ok_or_else(|| {
            let available = ports
                .iter()
                .map(|port| {
                    midi_input
                        .port_name(port)
                        .unwrap_or_else(|_| "(unknown)".to_string())
                })
                .collect::<Vec<_>>()
                .join(", ");
            anyhow!("KeyLab MIDI port not found; available ports: {available}")
        })?;

        let (sender, receiver) = mpsc::channel();
        let connection = midi_input
            .connect(
                &port,
                "rackforge-dls-keylab",
                move |_timestamp, message, _| {
                    if message.len() < 3 {
                        return;
                    }
                    match message[0] & 0xf0 {
                        0x90 if message[2] > 0 => {
                            let _ = sender.send(MidiEvent::NoteOn {
                                note: message[1] & 0x7f,
                                velocity: message[2] & 0x7f,
                            });
                        }
                        0x80 | 0x90 => {
                            let _ = sender.send(MidiEvent::NoteOff {
                                note: message[1] & 0x7f,
                            });
                        }
                        _ => {}
                    }
                },
                (),
            )
            .map_err(|error| anyhow!("could not connect to MIDI port {name}: {error}"))?;

        eprintln!("MIDI_CONNECTED port={name}");
        Ok((receiver, connection))
    }

    fn is_keylab_midi_port(name: &str) -> bool {
        let folded = name.to_ascii_lowercase();
        let is_keylab = folded.contains("kl essential") || folded.contains("keylab");
        let is_music_port = folded.contains("midi");
        let is_control_port = ["dinthru", "mcu", "hui", " alv"]
            .iter()
            .any(|marker| folded.contains(marker));
        is_keylab && is_music_port && !is_control_port
    }

    fn open_pcm() -> Result<PCM> {
        let pcm = PCM::new("hw:CARD=USB,DEV=0", Direction::Playback, false)
            .context("could not open Scarlett ALSA device hw:CARD=USB,DEV=0")?;
        {
            let hardware = HwParams::any(&pcm).context("could not query ALSA parameters")?;
            hardware
                .set_channels(CHANNELS)
                .context("could not set stereo output")?;
            hardware
                .set_rate(SAMPLE_RATE, ValueOr::Nearest)
                .context("could not set sample rate")?;
            hardware
                .set_format(Format::s32())
                .context("could not set signed 32-bit PCM")?;
            hardware
                .set_access(Access::RWInterleaved)
                .context("could not set interleaved access")?;
            hardware
                .set_period_size(PERIOD_FRAMES as i64, ValueOr::Nearest)
                .context("could not set ALSA period")?;
            hardware
                .set_buffer_size(BUFFER_FRAMES as i64)
                .context("could not set ALSA buffer")?;
            pcm.hw_params(&hardware)
                .context("could not apply ALSA parameters")?;
        }

        {
            let current = pcm.hw_params_current()?;
            eprintln!(
                "AUDIO_OPEN device=hw:CARD=USB,DEV=0 rate={} channels={} period={} buffer={}",
                current.get_rate()?,
                current.get_channels()?,
                current.get_period_size()?,
                current.get_buffer_size()?
            );
        }
        Ok(pcm)
    }
}

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    linux::run()
}
