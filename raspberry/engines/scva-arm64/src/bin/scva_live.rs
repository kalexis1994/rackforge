#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("artupy-scva-live is available on Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod linux {
    use alsa::pcm::{Access, Format, HwParams, PCM};
    use alsa::{Direction, ValueOr};
    use anyhow::{Context, Result, bail};
    use artupy_scva_bank::{ControlBankSet, INTERPOLATION_PHASE_COUNT, WaveBankSet};
    use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
    use midir::{Ignore, MidiInput, MidiInputConnection, MidiInputPort};
    use std::collections::BTreeMap;
    use std::env;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::mpsc::{self, Receiver, Sender};

    const OUTPUT_RATE: u32 = 48_000;
    const SOURCE_RATE: f64 = 32_000.0;
    const CHANNELS: usize = 2;
    const PERIOD_FRAMES: usize = 128;
    const BUFFER_FRAMES: i64 = 384;
    const FIRST_NOTE: u8 = 36;
    const LAST_NOTE: u8 = 96;
    const MAX_VOICES: usize = 16;
    const DEFAULT_MASTER_GAIN: f32 = 0.18;
    const RELEASE_FRAMES: usize = 2_400;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum PartialMode {
        Both,
        One,
        Two,
    }

    impl PartialMode {
        fn includes(self, partial: usize) -> bool {
            match self {
                Self::Both => true,
                Self::One => partial == 1,
                Self::Two => partial == 2,
            }
        }

        fn as_str(self) -> &'static str {
            match self {
                Self::Both => "both",
                Self::One => "1",
                Self::Two => "2",
            }
        }
    }

    struct Config {
        bank_directory: PathBuf,
        control_directory: PathBuf,
        partial_mode: PartialMode,
        master_gain: f32,
        dump_note: Option<(u8, PathBuf)>,
        rendered_bank: Option<PathBuf>,
    }

    #[derive(Clone, Copy, Debug)]
    enum MidiEvent {
        NoteOn { note: u8, velocity: u8 },
        NoteOff { note: u8 },
    }

    struct Voice {
        note: u8,
        samples: Arc<[f32]>,
        position: usize,
        gain: f32,
        release_remaining: Option<usize>,
    }

    pub fn run() -> Result<()> {
        let config = config_from_args()?;
        println!(
            "ARTUPY_SCVA_LIVE banks={} controls={} output=hw:CARD=USB,DEV=0 \
             rate={OUTPUT_RATE} period={PERIOD_FRAMES} partials={}",
            config.bank_directory.display(),
            config.control_directory.display(),
            config.partial_mode.as_str()
        );
        println!("MIX_CONFIG master_gain={:.3}", config.master_gain);

        let cache = if let Some(directory) = &config.rendered_bank {
            load_rendered_keyboard(directory)?
        } else {
            let banks = WaveBankSet::open(&config.bank_directory)?;
            let controls = ControlBankSet::open(&config.control_directory)?;
            preload_keyboard(&banks, &controls, config.partial_mode)?
        };
        println!(
            "CACHE_READY tone=0 notes={}..={} count={}",
            FIRST_NOTE,
            LAST_NOTE,
            cache.len()
        );
        if let Some((note, path)) = config.dump_note {
            write_debug_note(&cache, note, &path)?;
            return Ok(());
        }

        let (sender, receiver) = mpsc::channel();
        let (_midi_connection, midi_port_name) = connect_keylab(sender)?;
        println!("MIDI_READY port={midi_port_name:?}");

        let pcm = open_scarlett()?;
        println!(
            "AUDIO_READY device=\"hw:CARD=USB,DEV=0\" rate={OUTPUT_RATE} \
             channels={CHANNELS} format=S32_LE"
        );
        println!("READY_TO_PLAY");
        audio_loop(&pcm, &receiver, &cache, config.master_gain)
    }

    fn config_from_args() -> Result<Config> {
        let mut arguments = env::args_os().skip(1);
        let mut positional = Vec::new();
        let mut partial_mode = PartialMode::Both;
        let mut master_gain = DEFAULT_MASTER_GAIN;
        let mut dump_note = None;
        let mut rendered_bank = None;
        while let Some(argument) = arguments.next() {
            if argument == "--partial" {
                let value = arguments
                    .next()
                    .context("--partial requires both, 1, or 2")?;
                partial_mode = match value.to_string_lossy().as_ref() {
                    "both" => PartialMode::Both,
                    "1" => PartialMode::One,
                    "2" => PartialMode::Two,
                    _ => bail!("--partial requires both, 1, or 2"),
                };
            } else if argument == "--gain" {
                let value = arguments.next().context("--gain requires a value")?;
                master_gain = value
                    .to_string_lossy()
                    .parse()
                    .context("--gain must be a number")?;
                if !(0.0..=1.0).contains(&master_gain) {
                    bail!("--gain must be between 0.0 and 1.0");
                }
            } else if argument == "--dump-note" {
                let note = arguments
                    .next()
                    .context("--dump-note requires NOTE OUTPUT.wav")?
                    .to_string_lossy()
                    .parse::<u8>()
                    .context("--dump-note NOTE must be in 0..=127")?;
                let path = arguments
                    .next()
                    .context("--dump-note requires NOTE OUTPUT.wav")?;
                dump_note = Some((note, PathBuf::from(path)));
            } else if argument == "--rendered-bank" {
                let path = arguments
                    .next()
                    .context("--rendered-bank requires a directory")?;
                rendered_bank = Some(PathBuf::from(path));
            } else if argument.to_string_lossy().starts_with('-') {
                bail!("unknown option: {}", argument.to_string_lossy());
            } else {
                positional.push(argument);
            }
        }
        let (bank_directory, control_directory) = match positional.as_slice() {
            [] => (
                PathBuf::from("/home/kalex/artupy/share/scva"),
                PathBuf::from("/home/kalex/artupy/share/scva/control-v1"),
            ),
            [banks, controls] => (PathBuf::from(banks), PathBuf::from(controls)),
            _ => bail!(
                "usage: artupy-scva-live [--partial both|1|2] [--gain 0.0..1.0] \
                 [--dump-note NOTE OUTPUT.wav] \
                 [--rendered-bank DIRECTORY] \
                 [BANK_DIRECTORY CONTROL_DIRECTORY]"
            ),
        };
        Ok(Config {
            bank_directory,
            control_directory,
            partial_mode,
            master_gain,
            dump_note,
            rendered_bank,
        })
    }

    fn load_rendered_keyboard(directory: &PathBuf) -> Result<BTreeMap<u8, Arc<[f32]>>> {
        let mut cache = BTreeMap::new();
        for note in FIRST_NOTE..=LAST_NOTE {
            let path = directory.join(format!("note-{note:03}.wav"));
            let mut reader =
                WavReader::open(&path).with_context(|| format!("opening {}", path.display()))?;
            let specification = reader.spec();
            let channels = usize::from(specification.channels);
            if channels == 0 {
                bail!("{} declares zero channels", path.display());
            }
            let interleaved: Vec<f32> =
                match (specification.sample_format, specification.bits_per_sample) {
                    (SampleFormat::Float, 32) => reader
                        .samples::<f32>()
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                    (SampleFormat::Int, 16) => reader
                        .samples::<i16>()
                        .map(|sample| sample.map(|value| f32::from(value) / f32::from(i16::MAX)))
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                    _ => bail!(
                        "{} must be float32 or PCM16 WAV, found {:?}/{} bits",
                        path.display(),
                        specification.sample_format,
                        specification.bits_per_sample
                    ),
                };
            let mono: Vec<f32> = interleaved
                .chunks_exact(channels)
                .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
                .collect();
            if mono.is_empty() {
                bail!("{} contains no complete audio frames", path.display());
            }
            let step = f64::from(specification.sample_rate) / f64::from(OUTPUT_RATE);
            let output_frames = ((mono.len() as f64 / step).ceil() as usize).max(1);
            let mut resampled = Vec::with_capacity(output_frames);
            for output in 0..output_frames {
                let position = output as f64 * step;
                let index = position.floor() as usize;
                if index >= mono.len() {
                    break;
                }
                let fraction = (position - index as f64) as f32;
                let first = mono[index];
                let second = *mono.get(index + 1).unwrap_or(&first);
                resampled.push(first + (second - first) * fraction);
            }
            let peak = resampled
                .iter()
                .fold(0.0_f32, |maximum, sample| maximum.max(sample.abs()));
            let samples: Arc<[f32]> = Arc::from(resampled);
            cache.insert(note, samples);
            if note % 12 == 0 || note == LAST_NOTE {
                println!(
                    "RENDERED_CACHE note={note} source_rate={} frames={} peak={peak:.6}",
                    specification.sample_rate,
                    cache[&note].len()
                );
            }
        }
        println!(
            "RENDERED_BANK_READY directory={} notes={}",
            directory.display(),
            cache.len()
        );
        Ok(cache)
    }

    fn write_debug_note(cache: &BTreeMap<u8, Arc<[f32]>>, note: u8, path: &PathBuf) -> Result<()> {
        let samples = cache
            .get(&note)
            .with_context(|| format!("note {note} is outside the preloaded range"))?;
        let specification = WavSpec {
            channels: 1,
            sample_rate: OUTPUT_RATE,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(path, specification)
            .with_context(|| format!("creating {}", path.display()))?;
        for sample in samples.iter() {
            writer.write_sample(*sample)?;
        }
        writer.finalize()?;
        println!(
            "NOTE_DUMPED note={note} frames={} output={}",
            samples.len(),
            path.display()
        );
        Ok(())
    }

    fn remove_baseline_and_fade(samples: &mut [f64], note: u8) {
        if samples.is_empty() {
            return;
        }

        // FCE-DPCM reconstructs PCM with an accumulator. SCCore removes the
        // accumulator's slow baseline wander in its filter stage; subtracting
        // one average cannot remove a baseline that changes during the note.
        // Two gentle, key-tracked DC blockers reject only content well below
        // the played fundamental.
        let fundamental = 440.0 * 2_f64.powf((f64::from(note) - 69.0) / 12.0);
        let cutoff = (fundamental * 0.30).clamp(15.0, 700.0);
        let pole = (-2.0 * std::f64::consts::PI * cutoff / f64::from(OUTPUT_RATE)).exp();
        for _ in 0..2 {
            let mut previous_input = 0.0;
            let mut previous_output = 0.0;
            for sample in samples.iter_mut() {
                let input = *sample;
                let output = input - previous_input + pole * previous_output;
                *sample = output;
                previous_input = input;
                previous_output = output;
            }
        }

        let fade_frames = samples.len().min(240);
        for index in 0..fade_frames {
            let gain = index as f64 / fade_frames as f64;
            samples[index] *= gain;
            let end = samples.len() - 1 - index;
            samples[end] *= gain;
        }
    }

    fn preload_keyboard(
        banks: &WaveBankSet,
        controls: &ControlBankSet,
        partial_mode: PartialMode,
    ) -> Result<BTreeMap<u8, Arc<[f32]>>> {
        let mut rendered_notes: BTreeMap<u8, Vec<f64>> = BTreeMap::new();
        let mut global_peak = 0_f64;
        for note in FIRST_NOTE..=LAST_NOTE {
            let rendered = render_note(banks, controls, 0, note, partial_mode)?;
            let peak = rendered
                .iter()
                .fold(0_f64, |maximum, sample| maximum.max(sample.abs()));
            let rms = (rendered.iter().map(|sample| sample * sample).sum::<f64>()
                / rendered.len() as f64)
                .sqrt();
            global_peak = global_peak.max(peak);
            rendered_notes.insert(note, rendered);
            if note % 12 == 0 || note == LAST_NOTE {
                println!(
                    "CACHE_PROGRESS note={note} frames={} raw_peak={peak:.3} raw_rms={rms:.3}",
                    rendered_notes[&note].len()
                );
            }
        }
        if global_peak == 0.0 {
            bail!("preloaded keyboard rendered digital silence");
        }
        let scale = 0.90 / global_peak;
        let cache = rendered_notes
            .into_iter()
            .map(|(note, samples)| {
                let samples: Arc<[f32]> = Arc::from(
                    samples
                        .into_iter()
                        .map(|sample| (sample * scale) as f32)
                        .collect::<Vec<_>>(),
                );
                (note, samples)
            })
            .collect();
        println!("CACHE_NORMALIZED global_peak={global_peak:.3} scale={scale:.9}");
        Ok(cache)
    }

    fn interpolate_sccore(decoded: &[i32], position: f64, controls: &ControlBankSet) -> f64 {
        let index = position.floor() as isize;
        let fraction = position - position.floor();
        let phase = ((fraction * INTERPOLATION_PHASE_COUNT as f64) as usize)
            .min(INTERPOLATION_PHASE_COUNT - 1);
        let coefficients = controls
            .interpolation_coefficients(phase)
            .expect("phase was clamped to the interpolation table");
        let sample = |offset: isize| {
            let source = (index + offset).clamp(0, decoded.len() as isize - 1) as usize;
            f64::from(decoded[source])
        };
        sample(0) * f64::from(coefficients[0])
            + sample(1) * f64::from(coefficients[1])
            + sample(2) * f64::from(coefficients[2])
            + sample(3) * f64::from(coefficients[3])
    }

    fn render_note(
        banks: &WaveBankSet,
        controls: &ControlBankSet,
        tone: usize,
        note: u8,
        partial_mode: PartialMode,
    ) -> Result<Vec<f64>> {
        let resolution = controls.resolve(tone, usize::from(note))?;
        let mut rendered_partials: Vec<Vec<f64>> = Vec::new();

        for partial in &resolution.partials {
            if !partial_mode.includes(partial.partial) {
                continue;
            }
            let location = partial.sample_location;
            if location.reverse {
                bail!(
                    "tone {tone} note {note} partial {} uses unsupported reverse playback",
                    partial.partial
                );
            }
            let length = location
                .end
                .checked_sub(location.start)
                .context("sample end precedes sample start")?
                + 1;
            let decoded = banks
                .group(location.group)
                .segment(location.group_segment)?
                .decode_fce_dpcm(location.start, length, 0)?;
            let target_pitch_milli = i32::from(partial.mapped_note) * 1_000;
            let pitch_ratio =
                2_f64.powf(f64::from(target_pitch_milli - location.root_pitch_milli) / 12_000.0);
            let increment = pitch_ratio * SOURCE_RATE / f64::from(OUTPUT_RATE);
            let output_length = ((decoded.len() as f64 / increment).ceil() as usize)
                .min(OUTPUT_RATE as usize * 10)
                .max(1);
            let mut resampled = Vec::with_capacity(output_length);
            for output_index in 0..output_length {
                let position = output_index as f64 * increment;
                if position >= decoded.len() as f64 {
                    break;
                }
                resampled.push(
                    interpolate_sccore(&decoded, position, controls)
                        * (f64::from(partial.map_value) / 127.0),
                );
            }
            remove_baseline_and_fade(&mut resampled, note);
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
        if mixed.iter().all(|sample| *sample == 0.0) {
            bail!("tone {tone} note {note} rendered digital silence");
        }
        Ok(mixed)
    }

    fn is_keylab_midi(name: &str) -> bool {
        let folded = name.to_ascii_lowercase();
        (folded.contains("kl essential") || folded.contains("keylab"))
            && folded.contains("midi")
            && !folded.contains("dinthru")
            && !folded.contains("mcu")
            && !folded.contains("hui")
            && !folded.contains(" alv")
    }

    fn select_keylab_port(midi: &MidiInput) -> Result<(MidiInputPort, String)> {
        let mut matches = Vec::new();
        for port in midi.ports() {
            let name = midi.port_name(&port)?;
            if is_keylab_midi(&name) {
                matches.push((port, name));
            }
        }
        match matches.len() {
            0 => bail!("KeyLab main MIDI input was not found"),
            1 => Ok(matches.remove(0)),
            _ => bail!(
                "KeyLab MIDI input selection is ambiguous: {}",
                matches
                    .iter()
                    .map(|(_, name)| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    fn connect_keylab(sender: Sender<MidiEvent>) -> Result<(MidiInputConnection<()>, String)> {
        let mut midi = MidiInput::new("artupy-scva-live")?;
        midi.ignore(Ignore::None);
        let (port, name) = select_keylab_port(&midi)?;
        let connection = midi
            .connect(
                &port,
                "artupy-scva-live-input",
                move |_timestamp, message, _| {
                    if message.len() < 3 {
                        return;
                    }
                    let status = message[0] & 0xf0;
                    let note = message[1] & 0x7f;
                    let velocity = message[2] & 0x7f;
                    let event = match (status, velocity) {
                        (0x90, 1..=127) => Some(MidiEvent::NoteOn { note, velocity }),
                        (0x80 | 0x90, _) => Some(MidiEvent::NoteOff { note }),
                        _ => None,
                    };
                    if let Some(event) = event {
                        let _ = sender.send(event);
                    }
                },
                (),
            )
            .map_err(|error| anyhow::anyhow!("connecting KeyLab MIDI input: {error}"))?;
        Ok((connection, name))
    }

    fn open_scarlett() -> Result<PCM> {
        let pcm = PCM::new("hw:CARD=USB,DEV=0", Direction::Playback, false)
            .context("opening Scarlett ALSA playback")?;
        {
            let parameters = HwParams::any(&pcm)?;
            parameters.set_access(Access::RWInterleaved)?;
            parameters.set_format(Format::s32())?;
            parameters.set_channels(CHANNELS as u32)?;
            parameters.set_rate(OUTPUT_RATE, ValueOr::Nearest)?;
            parameters.set_period_size(PERIOD_FRAMES as i64, ValueOr::Nearest)?;
            parameters.set_buffer_size(BUFFER_FRAMES)?;
            pcm.hw_params(&parameters)?;
        }
        pcm.prepare()?;
        Ok(pcm)
    }

    fn audio_loop(
        pcm: &PCM,
        receiver: &Receiver<MidiEvent>,
        cache: &BTreeMap<u8, Arc<[f32]>>,
        master_gain: f32,
    ) -> Result<()> {
        let io = pcm.io_i32()?;
        let mut voices: Vec<Voice> = Vec::with_capacity(MAX_VOICES);
        let mut output = vec![0_i32; PERIOD_FRAMES * CHANNELS];
        let mut meter_frames = 0_usize;
        let mut meter_peak = 0_f32;
        let mut meter_clipped = 0_usize;

        loop {
            while let Ok(event) = receiver.try_recv() {
                match event {
                    MidiEvent::NoteOn { note, velocity } => {
                        let Some(samples) = cache.get(&note) else {
                            eprintln!(
                                "NOTE_IGNORED note={note} reason=outside_preloaded_range \
                                 range={FIRST_NOTE}..={LAST_NOTE}"
                            );
                            continue;
                        };
                        voices.retain(|voice| voice.note != note);
                        if voices.len() == MAX_VOICES {
                            voices.remove(0);
                        }
                        let velocity_gain = (f32::from(velocity) / 127.0).powf(0.7);
                        voices.push(Voice {
                            note,
                            samples: Arc::clone(samples),
                            position: 0,
                            gain: master_gain * velocity_gain,
                            release_remaining: None,
                        });
                        println!(
                            "NOTE_ON note={note} velocity={velocity} gain={:.3} voices={}",
                            master_gain * velocity_gain,
                            voices.len()
                        );
                    }
                    MidiEvent::NoteOff { note } => {
                        for voice in voices.iter_mut().filter(|voice| voice.note == note) {
                            voice.release_remaining = Some(RELEASE_FRAMES);
                        }
                        println!("NOTE_OFF note={note}");
                    }
                }
            }

            output.fill(0);
            for frame in 0..PERIOD_FRAMES {
                let mut mixed = 0_f32;
                for voice in &mut voices {
                    if let Some(sample) = voice.samples.get(voice.position) {
                        let release_gain = voice
                            .release_remaining
                            .map_or(1.0, |remaining| remaining as f32 / RELEASE_FRAMES as f32);
                        mixed += *sample * voice.gain * release_gain;
                        voice.position += 1;
                        if let Some(remaining) = &mut voice.release_remaining {
                            *remaining = remaining.saturating_sub(1);
                        }
                    }
                }
                meter_peak = meter_peak.max(mixed.abs());
                meter_clipped += usize::from(mixed.abs() > 0.95);
                meter_frames += 1;
                let sample = (mixed.clamp(-0.95, 0.95) * i32::MAX as f32) as i32;
                output[frame * 2] = sample;
                output[frame * 2 + 1] = sample;
            }
            voices.retain(|voice| {
                voice.position < voice.samples.len() && voice.release_remaining != Some(0)
            });
            if meter_frames >= OUTPUT_RATE as usize {
                println!(
                    "AUDIO_METER peak={meter_peak:.3} clipped={meter_clipped} voices={}",
                    voices.len()
                );
                meter_frames = 0;
                meter_peak = 0.0;
                meter_clipped = 0;
            }

            match io.writei(&output) {
                Ok(_) => {}
                Err(error) if error.errno() == libc::EPIPE => {
                    eprintln!("XRUN_RECOVERED");
                    pcm.prepare()?;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux::run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}
