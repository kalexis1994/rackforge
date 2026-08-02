#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("rackforge-scva-live is available on Linux only");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod linux {
    use alsa::pcm::{Access, Format, HwParams, PCM};
    use alsa::{Direction, ValueOr};
    use anyhow::{Context, Result, bail};
    use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
    use midir::{Ignore, MidiInput, MidiInputConnection, MidiInputPort};
    use rackforge_scva_bank::{
        ControlBankSet, INTERPOLATION_PHASE_COUNT, SCCORE_DECODER_ALIGNMENT, WaveBankSet,
        reverb::StereoReverb,
    };
    use std::collections::BTreeMap;
    use std::env;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::mpsc::{self, Receiver, Sender};

    const OUTPUT_RATE: u32 = 48_000;
    const ROM_RATE: f64 = 44_100.0;
    const CHANNELS: usize = 2;
    const PERIOD_FRAMES: usize = 128;
    const BUFFER_FRAMES: i64 = 384;
    const FIRST_NOTE: u8 = 36;
    const LAST_NOTE: u8 = 96;
    const MAX_VOICES: usize = 16;
    const DEFAULT_MASTER_GAIN: f32 = 0.18;
    const RELEASE_FRAMES: usize = 2_400;
    const DEBUG_DUMP_FRAMES: usize = OUTPUT_RATE as usize * 5;
    const ATTACK_FADE_FRAMES: usize = 240;
    type InterpolationTable = [[f32; 4]; INTERPOLATION_PHASE_COUNT];

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
        tone: usize,
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

    enum CachedPartial {
        Rendered {
            samples: Arc<[f32]>,
        },
        Sccore {
            decoded: Arc<[i32]>,
            increment: f64,
            loop_start: Option<f64>,
            level: f32,
            interpolation: Arc<InterpolationTable>,
        },
    }

    struct CachedNote {
        partials: Arc<[CachedPartial]>,
    }

    enum VoicePartial {
        Rendered {
            samples: Arc<[f32]>,
            position: usize,
        },
        Sccore {
            decoded: Arc<[i32]>,
            position: f64,
            increment: f64,
            loop_start: Option<f64>,
            has_looped: bool,
            age: usize,
            level: f32,
            interpolation: Arc<InterpolationTable>,
        },
    }

    struct Voice {
        note: u8,
        partials: Vec<VoicePartial>,
        gain: f32,
        release_remaining: Option<usize>,
    }

    impl VoicePartial {
        fn from_cached(partial: &CachedPartial) -> Self {
            match partial {
                CachedPartial::Rendered { samples } => Self::Rendered {
                    samples: Arc::clone(samples),
                    position: 0,
                },
                CachedPartial::Sccore {
                    decoded,
                    increment,
                    loop_start,
                    level,
                    interpolation,
                } => Self::Sccore {
                    decoded: Arc::clone(decoded),
                    position: 3.0,
                    increment: *increment,
                    loop_start: *loop_start,
                    has_looped: false,
                    age: 0,
                    level: *level,
                    interpolation: Arc::clone(interpolation),
                },
            }
        }

        fn next_sample(&mut self) -> Option<f32> {
            match self {
                Self::Rendered { samples, position } => {
                    let sample = samples.get(*position).copied();
                    *position += usize::from(sample.is_some());
                    sample
                }
                Self::Sccore {
                    decoded,
                    position,
                    increment,
                    loop_start,
                    has_looped,
                    age,
                    level,
                    interpolation,
                } => {
                    let end = decoded.len() as f64;
                    if *position >= end {
                        let start = (*loop_start)?;
                        let loop_length = end - start;
                        if loop_length <= 4.0 {
                            return None;
                        }
                        *position = start + (*position - end) % loop_length;
                        *has_looped = true;
                    }
                    let mut gain = (*age as f32 / ATTACK_FADE_FRAMES as f32).min(1.0);
                    if loop_start.is_none() {
                        let remaining = ((end - *position) / *increment) as f32;
                        gain *= (remaining / ATTACK_FADE_FRAMES as f32).clamp(0.0, 1.0);
                    }
                    let sample = interpolate_sccore_table(
                        decoded,
                        *position,
                        *loop_start,
                        *has_looped,
                        interpolation,
                    ) as f32
                        * *level
                        * gain;
                    *position += *increment;
                    *age = age.saturating_add(1);
                    Some(sample)
                }
            }
        }

        fn is_active(&self) -> bool {
            match self {
                Self::Rendered { samples, position } => *position < samples.len(),
                Self::Sccore {
                    decoded,
                    position,
                    loop_start,
                    ..
                } => loop_start.is_some() || *position < decoded.len() as f64,
            }
        }
    }

    pub fn run() -> Result<()> {
        let config = config_from_args()?;
        println!(
            "RACKFORGE_SCVA_LIVE banks={} controls={} output=hw:CARD=USB,DEV=0 \
             rate={OUTPUT_RATE} period={PERIOD_FRAMES} tone={} partials={}",
            config.bank_directory.display(),
            config.control_directory.display(),
            config.tone,
            config.partial_mode.as_str()
        );
        println!("MIX_CONFIG master_gain={:.3}", config.master_gain);

        let cache = if let Some(directory) = &config.rendered_bank {
            load_rendered_keyboard(directory)?
        } else {
            let banks = WaveBankSet::open(&config.bank_directory)?;
            let controls = ControlBankSet::open(&config.control_directory)?;
            preload_keyboard(&banks, &controls, config.tone, config.partial_mode)?
        };
        println!(
            "CACHE_READY tone={} notes={}..={} count={}",
            config.tone,
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
        let mut tone = 0;
        let mut partial_mode = PartialMode::Both;
        let mut master_gain = DEFAULT_MASTER_GAIN;
        let mut dump_note = None;
        let mut rendered_bank = None;
        while let Some(argument) = arguments.next() {
            if argument == "--tone" {
                let value = arguments.next().context("--tone requires an index")?;
                tone = value
                    .to_string_lossy()
                    .parse()
                    .context("--tone must be a non-negative integer")?;
            } else if argument == "--partial" {
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
                PathBuf::from("/home/kalex/rackforge/share/scva"),
                PathBuf::from("/home/kalex/rackforge/share/scva/control-v1"),
            ),
            [banks, controls] => (PathBuf::from(banks), PathBuf::from(controls)),
            _ => bail!(
                "usage: rackforge-scva-live [--tone INDEX] [--partial both|1|2] \
                 [--gain 0.0..1.0] \
                 [--dump-note NOTE OUTPUT.wav] \
                 [--rendered-bank DIRECTORY] \
                 [BANK_DIRECTORY CONTROL_DIRECTORY]"
            ),
        };
        Ok(Config {
            bank_directory,
            control_directory,
            tone,
            partial_mode,
            master_gain,
            dump_note,
            rendered_bank,
        })
    }

    fn load_rendered_keyboard(directory: &PathBuf) -> Result<BTreeMap<u8, CachedNote>> {
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
            let frame_count = samples.len();
            cache.insert(
                note,
                CachedNote {
                    partials: Arc::from([CachedPartial::Rendered { samples }]),
                },
            );
            if note % 12 == 0 || note == LAST_NOTE {
                println!(
                    "RENDERED_CACHE note={note} source_rate={} frames={} peak={peak:.6}",
                    specification.sample_rate, frame_count
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

    fn write_debug_note(cache: &BTreeMap<u8, CachedNote>, note: u8, path: &PathBuf) -> Result<()> {
        let cached = cache
            .get(&note)
            .with_context(|| format!("note {note} is outside the preloaded range"))?;
        let mut partials = cached
            .partials
            .iter()
            .map(VoicePartial::from_cached)
            .collect::<Vec<_>>();
        let specification = WavSpec {
            channels: 1,
            sample_rate: OUTPUT_RATE,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        };
        let mut writer = WavWriter::create(path, specification)
            .with_context(|| format!("creating {}", path.display()))?;
        for _ in 0..DEBUG_DUMP_FRAMES {
            let sample = partials
                .iter_mut()
                .filter_map(VoicePartial::next_sample)
                .sum::<f32>();
            writer.write_sample(sample)?;
        }
        writer.finalize()?;
        println!(
            "NOTE_DUMPED note={note} frames={} output={}",
            DEBUG_DUMP_FRAMES,
            path.display()
        );
        Ok(())
    }

    struct RenderedPartial {
        decoded: Vec<i32>,
        increment: f64,
        loop_start: Option<f64>,
        level: f64,
        preview: Vec<f64>,
    }

    fn mixed_peak(partials: &[RenderedPartial]) -> f64 {
        let frame_count = partials
            .iter()
            .map(|partial| partial.preview.len())
            .max()
            .unwrap_or(0);
        (0..frame_count).fold(0.0_f64, |peak, frame| {
            let mixed = partials
                .iter()
                .filter_map(|partial| partial.preview.get(frame))
                .sum::<f64>();
            peak.max(mixed.abs())
        })
    }

    fn preload_keyboard(
        banks: &WaveBankSet,
        controls: &ControlBankSet,
        tone: usize,
        partial_mode: PartialMode,
    ) -> Result<BTreeMap<u8, CachedNote>> {
        let interpolation: Arc<InterpolationTable> = Arc::new(std::array::from_fn(|phase| {
            controls
                .interpolation_coefficients(phase)
                .expect("phase is inside the interpolation table")
        }));
        let mut rendered_notes: BTreeMap<u8, Vec<RenderedPartial>> = BTreeMap::new();
        let mut global_peak = 0_f64;
        for note in FIRST_NOTE..=LAST_NOTE {
            let rendered = render_note(banks, controls, &interpolation, tone, note, partial_mode)?;
            let peak = mixed_peak(&rendered);
            let frames = rendered
                .iter()
                .map(|partial| partial.preview.len())
                .max()
                .unwrap_or(0);
            let loops = rendered
                .iter()
                .filter(|partial| partial.loop_start.is_some())
                .count();
            global_peak = global_peak.max(peak);
            rendered_notes.insert(note, rendered);
            if note % 12 == 0 || note == LAST_NOTE {
                println!(
                    "CACHE_PROGRESS note={note} frames={frames} partials={} loops={loops} \
                     raw_peak={peak:.3}",
                    rendered_notes[&note].len(),
                );
            }
        }
        if global_peak == 0.0 {
            bail!("preloaded keyboard rendered digital silence");
        }
        let scale = 0.90 / global_peak;
        let cache = rendered_notes
            .into_iter()
            .map(|(note, partials)| {
                let partials = partials
                    .into_iter()
                    .map(|partial| CachedPartial::Sccore {
                        decoded: Arc::from(partial.decoded),
                        increment: partial.increment,
                        loop_start: partial.loop_start,
                        level: (partial.level * scale) as f32,
                        interpolation: Arc::clone(&interpolation),
                    })
                    .collect::<Vec<_>>();
                (
                    note,
                    CachedNote {
                        partials: Arc::from(partials),
                    },
                )
            })
            .collect();
        println!("CACHE_NORMALIZED global_peak={global_peak:.3} scale={scale:.9}");
        Ok(cache)
    }

    fn interpolate_sccore_table(
        decoded: &[i32],
        position: f64,
        loop_start: Option<f64>,
        has_looped: bool,
        interpolation: &InterpolationTable,
    ) -> f64 {
        let index = position.floor() as isize;
        let fraction = position - position.floor();
        let phase = ((fraction * INTERPOLATION_PHASE_COUNT as f64) as usize)
            .min(INTERPOLATION_PHASE_COUNT - 1);
        let coefficients = interpolation[phase];
        let sample = |offset: isize| {
            let mut source = index + offset;
            if has_looped && let Some(start) = loop_start {
                let start = start.floor() as isize;
                if source < start {
                    source = decoded.len() as isize - (start - source);
                }
            }
            let source = source.clamp(0, decoded.len() as isize - 1) as usize;
            f64::from(decoded[source])
        };
        sample(-3) * f64::from(coefficients[0])
            + sample(-2) * f64::from(coefficients[1])
            + sample(-1) * f64::from(coefficients[2])
            + sample(0) * f64::from(coefficients[3])
    }

    fn render_note(
        banks: &WaveBankSet,
        controls: &ControlBankSet,
        interpolation: &InterpolationTable,
        tone: usize,
        note: u8,
        partial_mode: PartialMode,
    ) -> Result<Vec<RenderedPartial>> {
        let resolution = controls.resolve(tone, usize::from(note))?;
        let mut rendered_partials = Vec::new();

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
            let decoded = banks
                .group(location.group)
                .segment(location.group_segment)?
                .decode_sccore_window(location.start, location.end)?;
            let target_pitch_milli = i32::from(partial.mapped_note) * 1_000;
            let pitch_ratio =
                2_f64.powf(f64::from(target_pitch_milli - location.root_pitch_milli) / 12_000.0);
            let increment = pitch_ratio * ROM_RATE / f64::from(OUTPUT_RATE);
            let level = f64::from(partial.map_value) / 127.0;
            let output_length = (((decoded.len().saturating_sub(3)) as f64 / increment).ceil()
                as usize)
                .min(OUTPUT_RATE as usize * 10)
                .max(1);
            let mut preview = Vec::with_capacity(output_length);
            for output_index in 0..output_length {
                // SCCore primes four decoder samples and starts interpolation
                // with the cursor on the newest one.
                let position = 3.0 + output_index as f64 * increment;
                if position >= decoded.len() as f64 {
                    break;
                }
                let attack = (output_index as f64 / ATTACK_FADE_FRAMES as f64).min(1.0);
                preview.push(
                    interpolate_sccore_table(&decoded, position, None, false, interpolation)
                        * level
                        * attack,
                );
            }
            let loop_start = if location.looped {
                let aligned_start = location.start & !(SCCORE_DECODER_ALIGNMENT - 1);
                let source_offset = location
                    .loop_start
                    .checked_sub(aligned_start)
                    .context("sample loop precedes sample start")?;
                if source_offset + 4 >= decoded.len() {
                    bail!(
                        "tone {tone} note {note} partial {} has invalid output loop \
                         source_start={source_offset} source_end={}",
                        partial.partial,
                        decoded.len()
                    );
                }
                Some(source_offset as f64)
            } else {
                let fade_frames = preview.len().min(ATTACK_FADE_FRAMES);
                for frame in 0..fade_frames {
                    let index = preview.len() - 1 - frame;
                    preview[index] *= frame as f64 / fade_frames as f64;
                }
                None
            };
            let loop_period = loop_start.map(|start| (decoded.len() as f64 - start) / increment);
            println!(
                "PARTIAL_READY tone={tone} note={note} partial={} frames={} \
                 source_loop={} output_loop_period={}",
                partial.partial,
                preview.len(),
                loop_start
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_owned()),
                loop_period
                    .map(|value| format!("{value:.6}"))
                    .unwrap_or_else(|| "none".to_owned())
            );
            rendered_partials.push(RenderedPartial {
                decoded,
                increment,
                loop_start,
                level,
                preview,
            });
        }

        if rendered_partials.is_empty() {
            bail!("tone {tone} has no enabled partials");
        }
        if rendered_partials
            .iter()
            .all(|partial| partial.preview.iter().all(|sample| *sample == 0.0))
        {
            bail!("tone {tone} note {note} rendered digital silence");
        }
        Ok(rendered_partials)
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
        let mut midi = MidiInput::new("rackforge-scva-live")?;
        midi.ignore(Ignore::None);
        let (port, name) = select_keylab_port(&midi)?;
        let connection = midi
            .connect(
                &port,
                "rackforge-scva-live-input",
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
        cache: &BTreeMap<u8, CachedNote>,
        master_gain: f32,
    ) -> Result<()> {
        let io = pcm.io_i32()?;
        let mut voices: Vec<Voice> = Vec::with_capacity(MAX_VOICES);
        let mut output = vec![0_i32; PERIOD_FRAMES * CHANNELS];
        let mut meter_frames = 0_usize;
        let mut meter_peak = 0_f32;
        let mut meter_clipped = 0_usize;
        let mut reverb = StereoReverb::new(OUTPUT_RATE);

        loop {
            while let Ok(event) = receiver.try_recv() {
                match event {
                    MidiEvent::NoteOn { note, velocity } => {
                        let Some(cached) = cache.get(&note) else {
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
                        let partials = cached
                            .partials
                            .iter()
                            .map(VoicePartial::from_cached)
                            .collect();
                        voices.push(Voice {
                            note,
                            partials,
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
                    let release_gain = voice
                        .release_remaining
                        .map_or(1.0, |remaining| remaining as f32 / RELEASE_FRAMES as f32);
                    for partial in &mut voice.partials {
                        if let Some(sample) = partial.next_sample() {
                            mixed += sample * voice.gain * release_gain;
                        }
                    }
                    if let Some(remaining) = &mut voice.release_remaining {
                        *remaining = remaining.saturating_sub(1);
                    }
                }
                let (wet_left, wet_right) = reverb.process(mixed);
                let left = mixed + wet_left;
                let right = mixed + wet_right;
                meter_peak = meter_peak.max(left.abs()).max(right.abs());
                meter_clipped += usize::from(left.abs() > 0.95 || right.abs() > 0.95);
                meter_frames += 1;
                output[frame * 2] = (left.clamp(-0.95, 0.95) * i32::MAX as f32) as i32;
                output[frame * 2 + 1] = (right.clamp(-0.95, 0.95) * i32::MAX as f32) as i32;
            }
            voices.retain(|voice| {
                voice.release_remaining != Some(0)
                    && voice.partials.iter().any(VoicePartial::is_active)
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
