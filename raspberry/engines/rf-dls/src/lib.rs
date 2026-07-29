use std::fs;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

const DLS_DRUM_BANK: u32 = 0x8000_0000;
const CONN_SRC_NONE: u16 = 0x0000;
const CONN_SRC_EG2: u16 = 0x0005;
const CONN_DST_PITCH: u16 = 0x0003;
const CONN_DST_EG1_ATTACKTIME: u16 = 0x0206;
const CONN_DST_EG1_DECAYTIME: u16 = 0x0207;
const CONN_DST_EG1_RELEASETIME: u16 = 0x0209;
const CONN_DST_EG1_SUSTAINLEVEL: u16 = 0x020a;
const CONN_DST_EG2_ATTACKTIME: u16 = 0x030a;
const CONN_DST_EG2_DECAYTIME: u16 = 0x030b;
const CONN_DST_EG2_RELEASETIME: u16 = 0x030d;
const CONN_DST_EG2_SUSTAINLEVEL: u16 = 0x030e;

#[derive(Debug, Error)]
pub enum DlsError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid DLS structure: {0}")]
    Invalid(String),
    #[error("unsupported DLS feature: {0}")]
    Unsupported(String),
}

#[derive(Clone, Debug)]
struct Chunk {
    id: [u8; 4],
    kind: Option<[u8; 4]>,
    payload_start: usize,
    payload_len: usize,
    children: Vec<Chunk>,
}

impl Chunk {
    fn is(&self, id: &[u8; 4]) -> bool {
        &self.id == id
    }

    fn is_list(&self, kind: &[u8; 4]) -> bool {
        self.kind.as_ref() == Some(kind)
    }

    fn child(&self, id: &[u8; 4]) -> Option<&Self> {
        self.children.iter().find(|child| child.is(id))
    }

    fn list(&self, kind: &[u8; 4]) -> Option<&Self> {
        self.children.iter().find(|child| child.is_list(kind))
    }

    fn data<'a>(&self, bytes: &'a [u8]) -> &'a [u8] {
        &bytes[self.payload_start..self.payload_start + self.payload_len]
    }
}

fn fourcc(bytes: &[u8], offset: usize) -> [u8; 4] {
    bytes[offset..offset + 4].try_into().unwrap()
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn i16_at(bytes: &[u8], offset: usize) -> i16 {
    i16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn parse_chunks(
    bytes: &[u8],
    start: usize,
    end: usize,
    depth: usize,
) -> Result<Vec<Chunk>, DlsError> {
    if depth > 12 {
        return Err(DlsError::Invalid("RIFF nesting exceeds 12 levels".into()));
    }
    let mut chunks = Vec::new();
    let mut cursor = start;
    while cursor < end {
        if end - cursor < 8 {
            return Err(DlsError::Invalid(format!(
                "truncated chunk header at 0x{cursor:x}"
            )));
        }
        let id = fourcc(bytes, cursor);
        let size = u32_at(bytes, cursor + 4) as usize;
        let data_start = cursor + 8;
        let data_end = data_start
            .checked_add(size)
            .filter(|value| *value <= end)
            .ok_or_else(|| {
                DlsError::Invalid(format!(
                    "chunk {:?} at 0x{cursor:x} exceeds its container",
                    String::from_utf8_lossy(&id)
                ))
            })?;
        let (kind, payload_start, payload_len, children) = if id == *b"RIFF" || id == *b"LIST" {
            if size < 4 {
                return Err(DlsError::Invalid(format!(
                    "container at 0x{cursor:x} is shorter than its form type"
                )));
            }
            let kind = fourcc(bytes, data_start);
            (
                Some(kind),
                data_start + 4,
                size - 4,
                parse_chunks(bytes, data_start + 4, data_end, depth + 1)?,
            )
        } else {
            (None, data_start, size, Vec::new())
        };
        chunks.push(Chunk {
            id,
            kind,
            payload_start,
            payload_len,
            children,
        });
        cursor = data_end + (size & 1);
        if cursor > end {
            return Err(DlsError::Invalid(format!(
                "padding after chunk at 0x{:x} exceeds its container",
                data_start - 8
            )));
        }
    }
    Ok(chunks)
}

fn require_size(data: &[u8], minimum: usize, label: &str) -> Result<(), DlsError> {
    if data.len() < minimum {
        return Err(DlsError::Invalid(format!(
            "{label} has {} bytes; expected at least {minimum}",
            data.len()
        )));
    }
    Ok(())
}

fn info_name(container: &Chunk, bytes: &[u8]) -> String {
    container
        .list(b"INFO")
        .and_then(|info| info.child(b"INAM"))
        .map(|chunk| {
            let data = chunk.data(bytes);
            let end = data
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(data.len());
            String::from_utf8_lossy(&data[..end]).trim().to_owned()
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug)]
pub struct SampleLoop {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct SampleParams {
    pub unity_note: u16,
    pub fine_tune: i16,
    pub attenuation_db: f32,
    pub sample_loop: Option<SampleLoop>,
}

fn parse_wsmp(chunk: &Chunk, bytes: &[u8]) -> Result<SampleParams, DlsError> {
    let data = chunk.data(bytes);
    require_size(data, 20, "wsmp")?;
    let header_size = u32_at(data, 0) as usize;
    if !(20..=data.len()).contains(&header_size) {
        return Err(DlsError::Invalid(format!(
            "wsmp header size {header_size} is invalid"
        )));
    }
    let loop_count = u32_at(data, 16) as usize;
    let sample_loop = if loop_count == 0 {
        None
    } else {
        require_size(&data[header_size..], 16, "wsmp loop")?;
        let loop_size = u32_at(data, header_size) as usize;
        if loop_size < 16 || header_size + loop_size > data.len() {
            return Err(DlsError::Invalid("wsmp loop size is invalid".into()));
        }
        let loop_type = u32_at(data, header_size + 4);
        if loop_type > 1 {
            return Err(DlsError::Unsupported(format!(
                "sample loop type {loop_type}"
            )));
        }
        let start = u32_at(data, header_size + 8) as usize;
        let length = u32_at(data, header_size + 12) as usize;
        Some(SampleLoop {
            start,
            end: start
                .checked_add(length)
                .ok_or_else(|| DlsError::Invalid("sample loop overflows usize".into()))?,
        })
    };
    let attenuation_units = i32_at(data, 8) as f32 / 65_536.0;
    Ok(SampleParams {
        unity_note: u16_at(data, 4),
        fine_tune: i16_at(data, 6),
        attenuation_db: attenuation_units / 10.0,
        sample_loop,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct EnvelopeSpec {
    pub attack_seconds: f32,
    pub decay_seconds: f32,
    pub sustain_level: f32,
    pub release_seconds: f32,
}

impl Default for EnvelopeSpec {
    fn default() -> Self {
        Self {
            attack_seconds: 0.0,
            decay_seconds: 1_000.0,
            sustain_level: 1.0,
            release_seconds: 0.0,
        }
    }
}

impl EnvelopeSpec {
    fn pitch_default() -> Self {
        Self {
            attack_seconds: 0.0,
            decay_seconds: 1_000.0,
            sustain_level: 1.0,
            release_seconds: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PitchEnvelopeSpec {
    pub envelope: EnvelopeSpec,
    pub depth_cents: f32,
}

impl Default for PitchEnvelopeSpec {
    fn default() -> Self {
        Self {
            envelope: EnvelopeSpec::pitch_default(),
            depth_cents: 0.0,
        }
    }
}

fn timecents_to_seconds(raw: i32) -> f32 {
    if raw == i32::MIN {
        0.0
    } else {
        let timecents = raw as f32 / 65_536.0;
        2.0_f32.powf(timecents / 1_200.0).clamp(0.0, 1_000.0)
    }
}

fn sustain_level(raw: i32) -> f32 {
    (raw as f32 / 65_536.0 / 1_000.0).clamp(0.0, 1.0)
}

fn apply_articulation_connection(
    amplitude: &mut EnvelopeSpec,
    pitch: &mut PitchEnvelopeSpec,
    source: u16,
    control: u16,
    destination: u16,
    scale: i32,
) {
    match (source, control, destination) {
        (CONN_SRC_NONE, 0, CONN_DST_EG1_ATTACKTIME) => {
            amplitude.attack_seconds = timecents_to_seconds(scale)
        }
        (CONN_SRC_NONE, 0, CONN_DST_EG1_DECAYTIME) => {
            amplitude.decay_seconds = timecents_to_seconds(scale)
        }
        (CONN_SRC_NONE, 0, CONN_DST_EG1_RELEASETIME) => {
            amplitude.release_seconds = timecents_to_seconds(scale)
        }
        (CONN_SRC_NONE, 0, CONN_DST_EG1_SUSTAINLEVEL) => {
            amplitude.sustain_level = sustain_level(scale)
        }
        (CONN_SRC_NONE, 0, CONN_DST_EG2_ATTACKTIME) => {
            pitch.envelope.attack_seconds = timecents_to_seconds(scale)
        }
        (CONN_SRC_NONE, 0, CONN_DST_EG2_DECAYTIME) => {
            pitch.envelope.decay_seconds = timecents_to_seconds(scale)
        }
        (CONN_SRC_NONE, 0, CONN_DST_EG2_RELEASETIME) => {
            pitch.envelope.release_seconds = timecents_to_seconds(scale)
        }
        (CONN_SRC_NONE, 0, CONN_DST_EG2_SUSTAINLEVEL) => {
            pitch.envelope.sustain_level = sustain_level(scale)
        }
        (CONN_SRC_EG2, 0, CONN_DST_PITCH) => pitch.depth_cents = scale as f32 / 65_536.0,
        _ => {}
    }
}

fn parse_articulation(
    container: &Chunk,
    bytes: &[u8],
) -> Result<(EnvelopeSpec, PitchEnvelopeSpec), DlsError> {
    let mut amplitude = EnvelopeSpec::default();
    let mut pitch = PitchEnvelopeSpec::default();
    let Some(articulators) = container.list(b"lar2").or_else(|| container.list(b"lart")) else {
        return Ok((amplitude, pitch));
    };
    let Some(art) = articulators
        .child(b"art2")
        .or_else(|| articulators.child(b"art1"))
    else {
        return Ok((amplitude, pitch));
    };
    let data = art.data(bytes);
    require_size(data, 8, "articulator")?;
    let header_size = u32_at(data, 0) as usize;
    let count = u32_at(data, 4) as usize;
    let required = header_size
        .checked_add(count.saturating_mul(12))
        .ok_or_else(|| DlsError::Invalid("articulator size overflows".into()))?;
    if header_size < 8 || required > data.len() {
        return Err(DlsError::Invalid(
            "articulator connection table is truncated".into(),
        ));
    }
    for connection in 0..count {
        let offset = header_size + connection * 12;
        let source = u16_at(data, offset);
        let control = u16_at(data, offset + 2);
        let destination = u16_at(data, offset + 4);
        let scale = i32_at(data, offset + 8);
        apply_articulation_connection(
            &mut amplitude,
            &mut pitch,
            source,
            control,
            destination,
            scale,
        );
    }
    Ok((amplitude, pitch))
}

#[derive(Clone, Debug)]
pub struct Region {
    pub key_low: u16,
    pub key_high: u16,
    pub velocity_low: u16,
    pub velocity_high: u16,
    pub wave_index: usize,
    pub key_group: u16,
    pub sample_params: Option<SampleParams>,
}

#[derive(Clone, Debug)]
pub struct Instrument {
    pub name: String,
    pub bank: u32,
    pub program: u32,
    pub regions: Vec<Region>,
    pub envelope: EnvelopeSpec,
    pub pitch_envelope: PitchEnvelopeSpec,
}

impl Instrument {
    pub fn is_drum(&self) -> bool {
        self.bank & DLS_DRUM_BANK != 0
    }

    pub fn matching_regions(&self, note: u8, velocity: u8) -> impl Iterator<Item = &Region> {
        let note = u16::from(note);
        let velocity = u16::from(velocity);
        self.regions.iter().filter(move |region| {
            (region.key_low..=region.key_high).contains(&note)
                && (region.velocity_low..=region.velocity_high).contains(&velocity)
        })
    }
}

#[derive(Clone, Debug)]
pub struct Wave {
    pub name: String,
    pub sample_rate: u32,
    pub samples: Arc<[f32]>,
    pub sample_params: Option<SampleParams>,
}

#[derive(Debug)]
pub struct DlsBank {
    pub instruments: Vec<Instrument>,
    pub waves: Vec<Wave>,
}

impl DlsBank {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DlsError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|source| DlsError::Read {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, DlsError> {
        let roots = parse_chunks(bytes, 0, bytes.len(), 0)?;
        if roots.len() != 1 || !roots[0].is(b"RIFF") || !roots[0].is_list(b"DLS ") {
            return Err(DlsError::Invalid(
                "file is not a RIFF DLS collection".into(),
            ));
        }
        let root = &roots[0];
        let pool_table = root
            .child(b"ptbl")
            .ok_or_else(|| DlsError::Invalid("missing ptbl".into()))?;
        let pool = pool_table.data(bytes);
        require_size(pool, 8, "ptbl")?;
        let pool_header = u32_at(pool, 0) as usize;
        let cue_count = u32_at(pool, 4) as usize;
        if pool_header < 8 || pool_header + cue_count.saturating_mul(4) > pool.len() {
            return Err(DlsError::Invalid("ptbl cue table is truncated".into()));
        }
        let cues = (0..cue_count)
            .map(|index| u32_at(pool, pool_header + index * 4) as usize)
            .collect::<Vec<_>>();

        let wave_pool = root
            .list(b"wvpl")
            .ok_or_else(|| DlsError::Invalid("missing LIST wvpl".into()))?;
        let wave_pool_start = wave_pool.payload_start;
        let wave_chunks = wave_pool
            .children
            .iter()
            .filter(|chunk| chunk.is_list(b"wave"))
            .collect::<Vec<_>>();
        let mut waves_by_offset = std::collections::BTreeMap::new();
        for wave in wave_chunks {
            let relative = wave
                .payload_start
                .checked_sub(12)
                .and_then(|start| start.checked_sub(wave_pool_start))
                .ok_or_else(|| DlsError::Invalid("wave pool relative offset underflows".into()))?;
            waves_by_offset.insert(relative, parse_wave(wave, bytes)?);
        }
        let waves = cues
            .iter()
            .map(|cue| {
                waves_by_offset.get(cue).cloned().ok_or_else(|| {
                    DlsError::Invalid(format!("ptbl cue 0x{cue:x} does not select a wave"))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let instrument_list = root
            .list(b"lins")
            .ok_or_else(|| DlsError::Invalid("missing LIST lins".into()))?;
        let instruments = instrument_list
            .children
            .iter()
            .filter(|chunk| chunk.is_list(b"ins "))
            .map(|chunk| parse_instrument(chunk, bytes, waves.len()))
            .collect::<Result<Vec<_>, _>>()?;
        if instruments.is_empty() || waves.is_empty() {
            return Err(DlsError::Invalid(
                "DLS collection has no instruments or waves".into(),
            ));
        }
        Ok(Self { instruments, waves })
    }

    pub fn instrument(&self, bank: u32, program: u32) -> Option<&Instrument> {
        self.instruments
            .iter()
            .find(|instrument| instrument.bank == bank && instrument.program == program)
    }
}

fn parse_wave(chunk: &Chunk, bytes: &[u8]) -> Result<Wave, DlsError> {
    let format = chunk
        .child(b"fmt ")
        .ok_or_else(|| DlsError::Invalid("wave is missing fmt".into()))?
        .data(bytes);
    require_size(format, 16, "wave fmt")?;
    let format_tag = u16_at(format, 0);
    let channels = u16_at(format, 2);
    let sample_rate = u32_at(format, 4);
    let block_align = u16_at(format, 12);
    let bits = u16_at(format, 14);
    if format_tag != 1 || channels != 1 || bits != 16 || block_align != 2 {
        return Err(DlsError::Unsupported(format!(
            "wave format tag={format_tag} channels={channels} bits={bits} align={block_align}"
        )));
    }
    if sample_rate == 0 {
        return Err(DlsError::Invalid("wave sample rate is zero".into()));
    }
    let raw = chunk
        .child(b"data")
        .ok_or_else(|| DlsError::Invalid("wave is missing data".into()))?
        .data(bytes);
    if raw.len() % 2 != 0 {
        return Err(DlsError::Invalid("PCM16 wave has an odd data size".into()));
    }
    let samples = raw
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes(sample.try_into().unwrap()) as f32 / 32_768.0)
        .collect::<Vec<_>>();
    if samples.is_empty() {
        return Err(DlsError::Invalid("wave contains no samples".into()));
    }
    let sample_params = chunk
        .child(b"wsmp")
        .map(|wsmp| parse_wsmp(wsmp, bytes))
        .transpose()?;
    if let Some(sample_loop) = sample_params.and_then(|params| params.sample_loop)
        && (sample_loop.start >= sample_loop.end || sample_loop.end > samples.len())
    {
        return Err(DlsError::Invalid(format!(
            "wave loop {}..{} exceeds {} samples",
            sample_loop.start,
            sample_loop.end,
            samples.len()
        )));
    }
    Ok(Wave {
        name: info_name(chunk, bytes),
        sample_rate,
        samples: Arc::from(samples),
        sample_params,
    })
}

fn parse_instrument(
    chunk: &Chunk,
    bytes: &[u8],
    wave_count: usize,
) -> Result<Instrument, DlsError> {
    let header = chunk
        .child(b"insh")
        .ok_or_else(|| DlsError::Invalid("instrument is missing insh".into()))?
        .data(bytes);
    require_size(header, 12, "insh")?;
    let declared_regions = u32_at(header, 0) as usize;
    let bank = u32_at(header, 4);
    let program = u32_at(header, 8);
    let region_list = chunk
        .list(b"lrgn")
        .ok_or_else(|| DlsError::Invalid("instrument is missing LIST lrgn".into()))?;
    let regions = region_list
        .children
        .iter()
        .filter(|region| region.is_list(b"rgn ") || region.is_list(b"rgn2"))
        .map(|region| parse_region(region, bytes, wave_count))
        .collect::<Result<Vec<_>, _>>()?;
    if regions.len() != declared_regions {
        return Err(DlsError::Invalid(format!(
            "instrument declares {declared_regions} regions but contains {}",
            regions.len()
        )));
    }
    let (envelope, pitch_envelope) = parse_articulation(chunk, bytes)?;
    Ok(Instrument {
        name: info_name(chunk, bytes),
        bank,
        program,
        regions,
        envelope,
        pitch_envelope,
    })
}

fn parse_region(chunk: &Chunk, bytes: &[u8], wave_count: usize) -> Result<Region, DlsError> {
    let header = chunk
        .child(b"rgnh")
        .ok_or_else(|| DlsError::Invalid("region is missing rgnh".into()))?
        .data(bytes);
    require_size(header, 12, "rgnh")?;
    let wave_link = chunk
        .child(b"wlnk")
        .ok_or_else(|| DlsError::Invalid("region is missing wlnk".into()))?
        .data(bytes);
    require_size(wave_link, 12, "wlnk")?;
    let wave_index = u32_at(wave_link, 8) as usize;
    if wave_index >= wave_count {
        return Err(DlsError::Invalid(format!(
            "region selects wave {wave_index}, but collection has {wave_count}"
        )));
    }
    let sample_params = chunk
        .child(b"wsmp")
        .map(|wsmp| parse_wsmp(wsmp, bytes))
        .transpose()?;
    Ok(Region {
        key_low: u16_at(header, 0),
        key_high: u16_at(header, 2),
        velocity_low: u16_at(header, 4),
        velocity_high: u16_at(header, 6),
        key_group: u16_at(header, 10),
        wave_index,
        sample_params,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EnvelopePhase {
    Attack,
    Decay,
    Sustain,
    Release,
    Finished,
}

#[derive(Clone, Debug)]
struct Envelope {
    spec: EnvelopeSpec,
    phase: EnvelopePhase,
    level: f32,
    release_step: f32,
    sample_rate: f32,
}

impl Envelope {
    fn new(spec: EnvelopeSpec, sample_rate: u32) -> Self {
        let phase = if spec.attack_seconds <= 1.0 / sample_rate as f32 {
            EnvelopePhase::Decay
        } else {
            EnvelopePhase::Attack
        };
        Self {
            spec,
            phase,
            level: if phase == EnvelopePhase::Attack {
                0.0
            } else {
                1.0
            },
            release_step: 0.0,
            sample_rate: sample_rate as f32,
        }
    }

    fn note_off(&mut self) {
        if self.phase != EnvelopePhase::Finished && self.phase != EnvelopePhase::Release {
            let frames = (self.spec.release_seconds * self.sample_rate).max(1.0);
            self.release_step = self.level / frames;
            self.phase = EnvelopePhase::Release;
        }
    }

    fn next_gain(&mut self) -> f32 {
        match self.phase {
            EnvelopePhase::Attack => {
                self.level += 1.0 / (self.spec.attack_seconds * self.sample_rate).max(1.0);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.phase = EnvelopePhase::Decay;
                }
            }
            EnvelopePhase::Decay => {
                let step = (1.0 - self.spec.sustain_level)
                    / (self.spec.decay_seconds * self.sample_rate).max(1.0);
                self.level -= step;
                if self.level <= self.spec.sustain_level {
                    self.level = self.spec.sustain_level;
                    self.phase = EnvelopePhase::Sustain;
                }
            }
            EnvelopePhase::Sustain => {}
            EnvelopePhase::Release => {
                self.level -= self.release_step;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.phase = EnvelopePhase::Finished;
                }
            }
            EnvelopePhase::Finished => {}
        }
        self.level
    }
}

const AMPLITUDE_SILENCE_CENTIBELS: f32 = 960.0;

#[derive(Clone, Debug)]
struct AmplitudeEnvelope {
    spec: EnvelopeSpec,
    phase: EnvelopePhase,
    phase_frame: usize,
    phase_frames: usize,
    attenuation_centibels: f32,
    release_start_centibels: f32,
    sample_rate: f32,
}

impl AmplitudeEnvelope {
    fn new(spec: EnvelopeSpec, sample_rate: u32) -> Self {
        let mut envelope = Self {
            spec,
            phase: EnvelopePhase::Attack,
            phase_frame: 0,
            phase_frames: 0,
            attenuation_centibels: AMPLITUDE_SILENCE_CENTIBELS,
            release_start_centibels: AMPLITUDE_SILENCE_CENTIBELS,
            sample_rate: sample_rate as f32,
        };
        envelope.enter_attack();
        envelope
    }

    fn frames(&self, seconds: f32) -> usize {
        (seconds * self.sample_rate).max(0.0).round() as usize
    }

    fn enter_attack(&mut self) {
        self.phase_frames = self.frames(self.spec.attack_seconds);
        if self.phase_frames == 0 {
            self.attenuation_centibels = 0.0;
            self.enter_decay();
        }
    }

    fn enter_decay(&mut self) {
        self.phase = EnvelopePhase::Decay;
        self.phase_frame = 0;
        let sustain = self.sustain_centibels();
        self.phase_frames =
            self.frames(self.spec.decay_seconds * sustain / AMPLITUDE_SILENCE_CENTIBELS);
        if self.phase_frames == 0 || sustain == 0.0 {
            self.attenuation_centibels = sustain;
            self.phase = EnvelopePhase::Sustain;
        }
    }

    fn sustain_centibels(&self) -> f32 {
        ((1.0 - self.spec.sustain_level) * 1_000.0).clamp(0.0, AMPLITUDE_SILENCE_CENTIBELS)
    }

    fn note_off(&mut self) {
        if self.phase == EnvelopePhase::Finished || self.phase == EnvelopePhase::Release {
            return;
        }
        self.release_start_centibels = self.attenuation_centibels;
        let remaining = (AMPLITUDE_SILENCE_CENTIBELS - self.release_start_centibels).max(0.0);
        self.phase_frames =
            self.frames(self.spec.release_seconds * remaining / AMPLITUDE_SILENCE_CENTIBELS);
        self.phase_frame = 0;
        self.phase = if self.phase_frames == 0 {
            EnvelopePhase::Finished
        } else {
            EnvelopePhase::Release
        };
    }

    fn is_finished(&self) -> bool {
        self.phase == EnvelopePhase::Finished
    }

    fn next_gain(&mut self) -> f32 {
        match self.phase {
            EnvelopePhase::Attack => {
                self.phase_frame += 1;
                let gain = (self.phase_frame as f32 / self.phase_frames as f32).min(1.0);
                self.attenuation_centibels = if gain <= 0.0 {
                    AMPLITUDE_SILENCE_CENTIBELS
                } else {
                    (-200.0 * gain.log10()).clamp(0.0, AMPLITUDE_SILENCE_CENTIBELS)
                };
                if self.phase_frame >= self.phase_frames {
                    self.attenuation_centibels = 0.0;
                    self.enter_decay();
                }
                return gain;
            }
            EnvelopePhase::Decay => {
                self.phase_frame += 1;
                let progress = (self.phase_frame as f32 / self.phase_frames as f32).min(1.0);
                self.attenuation_centibels = self.sustain_centibels() * progress;
                if self.phase_frame >= self.phase_frames {
                    self.attenuation_centibels = self.sustain_centibels();
                    self.phase = EnvelopePhase::Sustain;
                }
            }
            EnvelopePhase::Sustain => {
                self.attenuation_centibels = self.sustain_centibels();
            }
            EnvelopePhase::Release => {
                self.phase_frame += 1;
                let progress = (self.phase_frame as f32 / self.phase_frames as f32).min(1.0);
                self.attenuation_centibels = self.release_start_centibels
                    + (AMPLITUDE_SILENCE_CENTIBELS - self.release_start_centibels) * progress;
                if self.phase_frame >= self.phase_frames {
                    self.attenuation_centibels = AMPLITUDE_SILENCE_CENTIBELS;
                    self.phase = EnvelopePhase::Finished;
                }
            }
            EnvelopePhase::Finished => {
                return 0.0;
            }
        }
        10.0_f32.powf(-self.attenuation_centibels / 200.0)
    }
}

#[derive(Clone, Debug)]
pub struct Voice {
    pub note: u8,
    samples: Arc<[f32]>,
    position: f64,
    base_increment: f64,
    sample_loop: Option<SampleLoop>,
    gain: f32,
    envelope: AmplitudeEnvelope,
    pitch_envelope: Envelope,
    pitch_depth_cents: f32,
    finished: bool,
}

impl Voice {
    pub fn new(
        bank: &DlsBank,
        instrument: &Instrument,
        region: &Region,
        note: u8,
        velocity: u8,
        output_rate: u32,
    ) -> Result<Self, DlsError> {
        let wave = &bank.waves[region.wave_index];
        let params = region.sample_params.or(wave.sample_params).ok_or_else(|| {
            DlsError::Unsupported(format!(
                "region for instrument {:?} has no wsmp parameters",
                instrument.name
            ))
        })?;
        if params.unity_note > 127 {
            return Err(DlsError::Invalid(format!(
                "unity note {} is outside MIDI range",
                params.unity_note
            )));
        }
        if let Some(sample_loop) = params.sample_loop
            && (sample_loop.start >= sample_loop.end || sample_loop.end > wave.samples.len())
        {
            return Err(DlsError::Invalid(format!(
                "region loop {}..{} exceeds wave {} length {}",
                sample_loop.start,
                sample_loop.end,
                region.wave_index,
                wave.samples.len()
            )));
        }
        let pitch_cents =
            (f64::from(note) - f64::from(params.unity_note)) * 100.0 + f64::from(params.fine_tune);
        let pitch = 2.0_f64.powf(pitch_cents / 1_200.0);
        let base_increment = f64::from(wave.sample_rate) / f64::from(output_rate) * pitch;
        let attenuation = 10.0_f32.powf(params.attenuation_db / 20.0);
        let velocity_gain = (f32::from(velocity) / 127.0).powf(0.7);
        Ok(Self {
            note,
            samples: Arc::clone(&wave.samples),
            position: 0.0,
            base_increment,
            sample_loop: params.sample_loop,
            gain: attenuation * velocity_gain,
            envelope: AmplitudeEnvelope::new(instrument.envelope, output_rate),
            pitch_envelope: Envelope::new(instrument.pitch_envelope.envelope, output_rate),
            pitch_depth_cents: instrument.pitch_envelope.depth_cents,
            finished: false,
        })
    }

    pub fn note_off(&mut self) {
        self.envelope.note_off();
        self.pitch_envelope.note_off();
    }

    pub fn is_finished(&self) -> bool {
        self.finished || self.envelope.is_finished()
    }

    pub fn next_sample(&mut self) -> f32 {
        if self.is_finished() {
            return 0.0;
        }
        let end = self
            .sample_loop
            .map_or(self.samples.len(), |looping| looping.end);
        if self.position >= end as f64 {
            if let Some(looping) = self.sample_loop {
                let length = (looping.end - looping.start) as f64;
                self.position =
                    looping.start as f64 + (self.position - looping.end as f64) % length;
            } else {
                self.finished = true;
                return 0.0;
            }
        }
        let index = self.position.floor() as usize;
        let fraction = (self.position - index as f64) as f32;
        let next = if index + 1 < end {
            index + 1
        } else {
            self.sample_loop.map_or(index, |looping| looping.start)
        };
        let sample = self.samples[index] + (self.samples[next] - self.samples[index]) * fraction;
        let pitch_cents = self.pitch_depth_cents * self.pitch_envelope.next_gain();
        let pitch_ratio = 2.0_f64.powf(f64::from(pitch_cents) / 1_200.0);
        self.position += self.base_increment * pitch_ratio;
        sample * self.gain * self.envelope.next_gain()
    }
}

pub fn voices_for_note(
    bank: &DlsBank,
    instrument: &Instrument,
    note: u8,
    velocity: u8,
    output_rate: u32,
) -> Result<Vec<Voice>, DlsError> {
    instrument
        .matching_regions(note, velocity)
        .map(|region| Voice::new(bank, instrument, region, note, velocity, output_rate))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecents_conversion_has_known_landmarks() {
        assert!((timecents_to_seconds(0) - 1.0).abs() < 1e-6);
        assert!((timecents_to_seconds(1_200 * 65_536) - 2.0).abs() < 1e-5);
        assert_eq!(timecents_to_seconds(i32::MIN), 0.0);
    }

    #[test]
    fn default_envelope_is_finite() {
        let mut envelope = Envelope::new(EnvelopeSpec::default(), 48_000);
        for _ in 0..48_000 {
            assert!(envelope.next_gain().is_finite());
        }
        envelope.note_off();
        for _ in 0..48_000 {
            assert!(envelope.next_gain().is_finite());
        }
        assert_eq!(envelope.phase, EnvelopePhase::Finished);
    }

    #[test]
    fn parses_eg2_pitch_depth_in_cents() {
        let mut amplitude = EnvelopeSpec::default();
        let mut pitch = PitchEnvelopeSpec::default();
        apply_articulation_connection(
            &mut amplitude,
            &mut pitch,
            CONN_SRC_EG2,
            CONN_SRC_NONE,
            CONN_DST_PITCH,
            102 * 65_536,
        );
        assert_eq!(pitch.depth_cents, 102.0);
    }

    #[test]
    fn pitch_envelope_modulates_voice_playback_rate() {
        let bank = DlsBank {
            instruments: vec![Instrument {
                name: "Pitch envelope".into(),
                bank: 0,
                program: 0,
                regions: vec![Region {
                    key_low: 0,
                    key_high: 127,
                    velocity_low: 0,
                    velocity_high: 127,
                    wave_index: 0,
                    key_group: 0,
                    sample_params: Some(SampleParams {
                        unity_note: 60,
                        fine_tune: 0,
                        attenuation_db: 0.0,
                        sample_loop: None,
                    }),
                }],
                envelope: EnvelopeSpec::default(),
                pitch_envelope: PitchEnvelopeSpec {
                    envelope: EnvelopeSpec::pitch_default(),
                    depth_cents: 1_200.0,
                },
            }],
            waves: vec![Wave {
                name: "Synthetic".into(),
                sample_rate: 48_000,
                samples: Arc::from(vec![0.0; 32]),
                sample_params: None,
            }],
        };
        let instrument = &bank.instruments[0];
        let mut voice =
            Voice::new(&bank, instrument, &instrument.regions[0], 60, 127, 48_000).unwrap();
        voice.next_sample();
        assert!((voice.position - 2.0).abs() < 1e-6);
    }

    #[test]
    fn positive_sample_fine_tune_raises_playback_rate() {
        let bank = DlsBank {
            instruments: vec![Instrument {
                name: "Fine tune".into(),
                bank: 0,
                program: 0,
                regions: vec![Region {
                    key_low: 0,
                    key_high: 127,
                    velocity_low: 0,
                    velocity_high: 127,
                    wave_index: 0,
                    key_group: 0,
                    sample_params: Some(SampleParams {
                        unity_note: 60,
                        fine_tune: 100,
                        attenuation_db: 0.0,
                        sample_loop: None,
                    }),
                }],
                envelope: EnvelopeSpec::default(),
                pitch_envelope: PitchEnvelopeSpec::default(),
            }],
            waves: vec![Wave {
                name: "Synthetic".into(),
                sample_rate: 48_000,
                samples: Arc::from(vec![0.0; 32]),
                sample_params: None,
            }],
        };
        let instrument = &bank.instruments[0];
        let mut voice =
            Voice::new(&bank, instrument, &instrument.regions[0], 60, 127, 48_000).unwrap();
        voice.next_sample();
        assert!((voice.position - 2.0_f64.powf(1.0 / 12.0)).abs() < 1e-6);
    }

    #[test]
    fn amplitude_release_falls_linearly_in_centibels() {
        let mut envelope = AmplitudeEnvelope::new(
            EnvelopeSpec {
                attack_seconds: 0.0,
                decay_seconds: 0.0,
                sustain_level: 1.0,
                release_seconds: 1.0,
            },
            100,
        );
        assert_eq!(envelope.next_gain(), 1.0);
        envelope.note_off();
        let mut halfway_gain = 1.0;
        for _ in 0..50 {
            halfway_gain = envelope.next_gain();
        }
        assert!((halfway_gain - 10.0_f32.powf(-2.4)).abs() < 1e-5);
    }
}
