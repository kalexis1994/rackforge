use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

pub const SEGMENT_SIZE: usize = 1024 * 1024;
pub const SCALE_TABLE_SIZE: usize = SEGMENT_SIZE / 32;
pub const SAMPLE_RECORD_SIZE: usize = 0x16;
pub const SAMPLE_RECORD_COUNT: usize = 4_259;
pub const WAVE_MAP_RECORD_SIZE: usize = 0x8c;
pub const WAVE_MAP_RECORD_COUNT: usize = 1_175;
pub const TONE_RECORD_SIZE: usize = 0x100;
pub const TONE_RECORD_COUNT: usize = 2_363;
pub const INTERPOLATION_PHASE_COUNT: usize = 128;
pub const INTERPOLATION_TAP_COUNT: usize = 4;

const KNOWN_112_HASHES: [&str; 4] = [
    "05a36e2e354611e667b643d619c9c1d2a2f0836bd585189e061b82f27b827385",
    "0e5edc077367165751464ee8d9028a5c6b23cf57ad69254d3ff687da5c2de0a6",
    "bc96fb86fae38ce1b187e48b75e3bcbca444821522deb7b5105821759b51d391",
    "5e7c4e32963da835db54e3663221606ee875bf1b20a0c4f0d57ebacdc5085be2",
];

#[derive(Debug, Error)]
pub enum BankError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{file} has size 0x{actual:x}; expected exactly 0x{expected:x}")]
    InvalidSize {
        file: &'static str,
        actual: usize,
        expected: usize,
    },
    #[error("{file} segment {segment} has marker {actual:?}; expected {expected:?}")]
    InvalidMarker {
        file: &'static str,
        segment: usize,
        actual: String,
        expected: &'static str,
    },
    #[error("{file} segment {segment} has date {actual:?}; expected {expected:?}")]
    InvalidDate {
        file: &'static str,
        segment: usize,
        actual: String,
        expected: &'static str,
    },
    #[error("group {group} has no segment {segment}; available range is 0..{count}")]
    InvalidSegment {
        group: GroupId,
        segment: usize,
        count: usize,
    },
    #[error("sample descriptor selects flat Wave ROM segment {segment}; valid range is 0..24")]
    InvalidFlatSegment { segment: usize },
    #[error(
        "DPCM range 0x{start:x}..0x{end:x} is invalid; sample data must stay \
         inside 0x{minimum:x}..0x{maximum:x}"
    )]
    InvalidDpcmRange {
        start: usize,
        end: usize,
        minimum: usize,
        maximum: usize,
    },
    #[error("DPCM accumulator exceeds i32 at sample {sample}")]
    DpcmOverflow { sample: usize },
    #[error("{file} has SHA-256 {actual}; expected the supported 1.1.2 hash {expected}")]
    InvalidHash {
        file: &'static str,
        actual: String,
        expected: &'static str,
    },
    #[error("tone {tone} is outside 0..{maximum}")]
    InvalidTone { tone: usize, maximum: usize },
    #[error("MIDI note {note} is outside 0..=127")]
    InvalidNote { note: usize },
    #[error(
        "tone {tone} partial {partial} references wave map {wave_map}; \
         supported range is 0..{maximum}"
    )]
    InvalidWaveMap {
        tone: usize,
        partial: usize,
        wave_map: usize,
        maximum: usize,
    },
    #[error("tone {tone} partial {partial} wave map {wave_map} has no range for MIDI note {note}")]
    MissingKeyRange {
        tone: usize,
        partial: usize,
        wave_map: usize,
        note: usize,
    },
    #[error(
        "tone {tone} partial {partial} wave map {wave_map} references sample {sample}; \
         supported range is 0..{maximum}"
    )]
    InvalidSample {
        tone: usize,
        partial: usize,
        wave_map: usize,
        sample: usize,
        maximum: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupId {
    Sc88Rev200,
    RomMakeA,
    RomMakeB,
    Sc8820,
}

impl GroupId {
    pub const ALL: [Self; 4] = [
        Self::Sc88Rev200,
        Self::RomMakeA,
        Self::RomMakeB,
        Self::Sc8820,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sc88Rev200 => "sc88-rev200",
            Self::RomMakeA => "rom-make-a",
            Self::RomMakeB => "rom-make-b",
            Self::Sc8820 => "sc8820",
        }
    }

    pub const fn from_flat_segment(segment: usize) -> Option<(Self, usize)> {
        match segment {
            0..=7 => Some((Self::Sc88Rev200, segment)),
            8..=15 => Some((Self::RomMakeA, segment - 8)),
            16..=19 => Some((Self::RomMakeB, segment - 16)),
            20..=23 => Some((Self::Sc8820, segment - 20)),
            _ => None,
        }
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for GroupId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "sc88-rev200" => Ok(Self::Sc88Rev200),
            "rom-make-a" => Ok(Self::RomMakeA),
            "rom-make-b" => Ok(Self::RomMakeB),
            "sc8820" => Ok(Self::Sc8820),
            _ => Err(format!(
                "unknown group {value:?}; expected sc88-rev200, rom-make-a, \
                 rom-make-b, or sc8820"
            )),
        }
    }
}

#[derive(Clone, Copy)]
struct GroupSpec {
    id: GroupId,
    file_name: &'static str,
    marker: &'static str,
    date: &'static str,
    segment_count: usize,
}

const GROUP_SPECS: [GroupSpec; 4] = [
    GroupSpec {
        id: GroupId::Sc88Rev200,
        file_name: "wave_1994_ver200_8mib.bin",
        marker: "ver200",
        date: "1994-12-08",
        segment_count: 8,
    },
    GroupSpec {
        id: GroupId::RomMakeA,
        file_name: "wave_1996_rom_make_a_8mib.bin",
        marker: "rom_make",
        date: "1996-06-16",
        segment_count: 8,
    },
    GroupSpec {
        id: GroupId::RomMakeB,
        file_name: "wave_1996_rom_make_b_4mib.bin",
        marker: "rom_make",
        date: "1996-06-16",
        segment_count: 4,
    },
    GroupSpec {
        id: GroupId::Sc8820,
        file_name: "wave_1999_sc8820_4mib.bin",
        marker: "8820_wv0",
        date: "1999-08-17",
        segment_count: 4,
    },
];

#[derive(Clone, Debug)]
pub struct SegmentHeader {
    pub marker: String,
    pub date: String,
    pub config_words: [u32; 3],
}

#[derive(Debug)]
pub struct WaveGroup {
    id: GroupId,
    file_name: &'static str,
    sha256: String,
    known_112: bool,
    data: Box<[u8]>,
    headers: Vec<SegmentHeader>,
}

impl WaveGroup {
    pub fn id(&self) -> GroupId {
        self.id
    }

    pub fn file_name(&self) -> &'static str {
        self.file_name
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn is_known_112(&self) -> bool {
        self.known_112
    }

    pub fn segment_count(&self) -> usize {
        self.headers.len()
    }

    pub fn header(&self, segment: usize) -> Result<&SegmentHeader, BankError> {
        self.headers.get(segment).ok_or(BankError::InvalidSegment {
            group: self.id,
            segment,
            count: self.segment_count(),
        })
    }

    pub fn segment(&self, segment: usize) -> Result<WaveSegment<'_>, BankError> {
        self.header(segment)?;
        let start = segment * SEGMENT_SIZE;
        Ok(WaveSegment {
            group: self.id,
            index: segment,
            data: &self.data[start..start + SEGMENT_SIZE],
        })
    }
}

#[derive(Debug)]
pub struct WaveBankSet {
    groups: Vec<WaveGroup>,
}

struct ControlSpec {
    file_name: &'static str,
    size: usize,
    sha256: &'static str,
}

const SAMPLE_SPEC: ControlSpec = ControlSpec {
    file_name: "sample-descriptors.bin",
    size: 0x16e04,
    sha256: "d0245487637b5cd0cffb98853f30ae71b84a7c03c85c3fdc338af2ed42051426",
};
const WAVE_MAP_SPEC: ControlSpec = ControlSpec {
    file_name: "wave-maps.bin",
    size: 0x28294,
    sha256: "d25f65b28e4102123e72ecd4050e213b40d46f28abaec48f89ad086faa7039b0",
};
const TONE_SPEC: ControlSpec = ControlSpec {
    file_name: "tones.bin",
    size: 0x93b00,
    sha256: "9a4f4ae5017f338c6bca2356e8236aa5a501c4388b4a490499e3765a04839104",
};
const INTERPOLATION_SPEC: ControlSpec = ControlSpec {
    file_name: "interpolation-coefficients.bin",
    size: INTERPOLATION_PHASE_COUNT * INTERPOLATION_TAP_COUNT * size_of::<f32>(),
    sha256: "7fb7907e1d10d9f55b28eb150811e9d512ca46b2e8796506fc11f2e6a28cee81",
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialResolution {
    pub partial: usize,
    pub wave_map: usize,
    pub wave_map_name: String,
    pub key_bias: u8,
    pub mapped_note: u8,
    pub slot: usize,
    pub upper_note: u8,
    pub map_value: u8,
    pub sample: usize,
    pub descriptor: [u8; SAMPLE_RECORD_SIZE],
    pub sample_location: SampleLocation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToneResolution {
    pub tone: usize,
    pub tone_name: String,
    pub note: u8,
    pub partial_mask: u8,
    pub partials: Vec<PartialResolution>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleLocation {
    pub flat_segment: usize,
    pub group: GroupId,
    pub group_segment: usize,
    pub flags: u8,
    pub wave_selector: u32,
    pub start: usize,
    pub loop_start: usize,
    pub end: usize,
    pub looped: bool,
    pub reverse: bool,
    pub root_key: u8,
    pub fine_tune_word: u16,
    pub root_pitch_milli: i32,
}

impl SampleLocation {
    pub fn parse(descriptor: &[u8; SAMPLE_RECORD_SIZE]) -> Option<Self> {
        let flat_segment = usize::from(descriptor[0] & 0x7f);
        let (group, group_segment) = GroupId::from_flat_segment(flat_segment)?;
        let flags = descriptor[10];
        let first = packed_address(descriptor, 1);
        let middle = packed_address(descriptor, 7);
        let last = packed_address(descriptor, 11);
        let reverse = flags & 4 != 0;
        let looped = flags & 2 != 0;
        let fine_tune_word = u16::from_le_bytes(descriptor[4..6].try_into().unwrap());
        let root_key = descriptor[6];
        let root_pitch_milli = i32::from(root_key) * 1_000 - i32::from(fine_tune_word) + 0x400;
        let (start, loop_start, end) = if reverse {
            if looped {
                let boundary = first.saturating_sub(1);
                (last, boundary, boundary)
            } else {
                (last, middle, first)
            }
        } else {
            (first, middle, last)
        };
        let wave_selector = ((u32::from(flags & 4) + (8 + u32::from(flags & 1)) * 8) << 9)
            + u32::from(descriptor[0] & 0x7f);

        Some(Self {
            flat_segment,
            group,
            group_segment,
            flags,
            wave_selector,
            start,
            loop_start,
            end,
            looped,
            reverse,
            root_key,
            fine_tune_word,
            root_pitch_milli,
        })
    }
}

#[derive(Debug)]
pub struct ControlBankSet {
    samples: Box<[u8]>,
    wave_maps: Box<[u8]>,
    tones: Box<[u8]>,
    interpolation: Box<[u8]>,
}

impl ControlBankSet {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, BankError> {
        let directory = directory.as_ref();
        Ok(Self {
            samples: read_exact_control(directory, &SAMPLE_SPEC)?,
            wave_maps: read_exact_control(directory, &WAVE_MAP_SPEC)?,
            tones: read_exact_control(directory, &TONE_SPEC)?,
            interpolation: read_exact_control(directory, &INTERPOLATION_SPEC)?,
        })
    }

    pub fn interpolation_coefficients(
        &self,
        phase: usize,
    ) -> Option<[f32; INTERPOLATION_TAP_COUNT]> {
        if phase >= INTERPOLATION_PHASE_COUNT {
            return None;
        }
        let start = phase * INTERPOLATION_TAP_COUNT * size_of::<f32>();
        Some(std::array::from_fn(|tap| {
            let offset = start + tap * size_of::<f32>();
            f32::from_le_bytes(
                self.interpolation[offset..offset + size_of::<f32>()]
                    .try_into()
                    .unwrap(),
            )
        }))
    }

    pub fn resolve(&self, tone: usize, note: usize) -> Result<ToneResolution, BankError> {
        if tone >= TONE_RECORD_COUNT {
            return Err(BankError::InvalidTone {
                tone,
                maximum: TONE_RECORD_COUNT - 1,
            });
        }
        if note > 127 {
            return Err(BankError::InvalidNote { note });
        }

        let tone_record = record(&self.tones, tone, TONE_RECORD_SIZE);
        let tone_name = fixed_name(&tone_record[..12]);
        let partial_mask = tone_record[0x16];
        let mut partials = Vec::with_capacity(2);

        for partial in 0..2 {
            if partial_mask & (1 << partial) == 0 {
                continue;
            }
            let partial_offset = 0x24 + partial * 0x6e;
            let wave_map = usize::from(u16::from_le_bytes(
                tone_record[partial_offset + 2..partial_offset + 4]
                    .try_into()
                    .unwrap(),
            ));
            if wave_map >= WAVE_MAP_RECORD_COUNT {
                return Err(BankError::InvalidWaveMap {
                    tone,
                    partial: partial + 1,
                    wave_map,
                    maximum: WAVE_MAP_RECORD_COUNT - 1,
                });
            }
            let key_bias = tone_record[partial_offset + 4];
            let mapped_note = note
                .saturating_add(0x40)
                .saturating_sub(usize::from(key_bias))
                .min(127) as u8;
            let map_record = record(&self.wave_maps, wave_map, WAVE_MAP_RECORD_SIZE);
            let slot = map_record[0x0c..0x2c]
                .iter()
                .position(|upper| *upper >= mapped_note)
                .ok_or(BankError::MissingKeyRange {
                    tone,
                    partial: partial + 1,
                    wave_map,
                    note: usize::from(mapped_note),
                })?;
            let sample = usize::from(u16::from_le_bytes(
                map_record[0x2c + slot * 2..0x2e + slot * 2]
                    .try_into()
                    .unwrap(),
            ));
            if sample >= SAMPLE_RECORD_COUNT {
                return Err(BankError::InvalidSample {
                    tone,
                    partial: partial + 1,
                    wave_map,
                    sample,
                    maximum: SAMPLE_RECORD_COUNT - 1,
                });
            }
            let descriptor: [u8; SAMPLE_RECORD_SIZE] =
                record(&self.samples, sample, SAMPLE_RECORD_SIZE)
                    .try_into()
                    .unwrap();
            let sample_location =
                SampleLocation::parse(&descriptor).ok_or(BankError::InvalidFlatSegment {
                    segment: usize::from(descriptor[0] & 0x7f),
                })?;
            partials.push(PartialResolution {
                partial: partial + 1,
                wave_map,
                wave_map_name: fixed_name(&map_record[..12]),
                key_bias,
                mapped_note,
                slot,
                upper_note: map_record[0x0c + slot],
                map_value: map_record[0x6c + slot],
                sample,
                descriptor,
                sample_location,
            });
        }

        Ok(ToneResolution {
            tone,
            tone_name,
            note: note as u8,
            partial_mask,
            partials,
        })
    }
}

fn read_exact_control(directory: &Path, spec: &ControlSpec) -> Result<Box<[u8]>, BankError> {
    let path = directory.join(spec.file_name);
    let bytes = fs::read(&path).map_err(|source| BankError::Read {
        path: path.display().to_string(),
        source,
    })?;
    if bytes.len() != spec.size {
        return Err(BankError::InvalidSize {
            file: spec.file_name,
            actual: bytes.len(),
            expected: spec.size,
        });
    }
    let actual = sha256_hex(&bytes);
    if actual != spec.sha256 {
        return Err(BankError::InvalidHash {
            file: spec.file_name,
            actual,
            expected: spec.sha256,
        });
    }
    Ok(bytes.into_boxed_slice())
}

fn record(bytes: &[u8], index: usize, size: usize) -> &[u8] {
    &bytes[index * size..(index + 1) * size]
}

fn fixed_name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches([' ', '\0'])
        .to_owned()
}

fn packed_address(bytes: &[u8; SAMPLE_RECORD_SIZE], offset: usize) -> usize {
    (usize::from(bytes[offset] & 0x0f) << 16)
        | (usize::from(bytes[offset + 1]) << 8)
        | usize::from(bytes[offset + 2])
}

impl WaveBankSet {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, BankError> {
        let directory = directory.as_ref();
        let mut groups = Vec::with_capacity(GROUP_SPECS.len());

        for (spec_index, spec) in GROUP_SPECS.iter().enumerate() {
            let path = directory.join(spec.file_name);
            let data = fs::read(&path).map_err(|source| BankError::Read {
                path: path.display().to_string(),
                source,
            })?;
            let expected_size = spec.segment_count * SEGMENT_SIZE;
            if data.len() != expected_size {
                return Err(BankError::InvalidSize {
                    file: spec.file_name,
                    actual: data.len(),
                    expected: expected_size,
                });
            }

            let mut headers = Vec::with_capacity(spec.segment_count);
            for segment in 0..spec.segment_count {
                let start = segment * SEGMENT_SIZE;
                let bytes = &data[start..start + SEGMENT_SIZE];
                let header = parse_header(bytes);
                if header.marker != spec.marker {
                    return Err(BankError::InvalidMarker {
                        file: spec.file_name,
                        segment,
                        actual: header.marker,
                        expected: spec.marker,
                    });
                }
                if header.date != spec.date {
                    return Err(BankError::InvalidDate {
                        file: spec.file_name,
                        segment,
                        actual: header.date,
                        expected: spec.date,
                    });
                }
                headers.push(header);
            }

            let sha256 = sha256_hex(&data);
            groups.push(WaveGroup {
                id: spec.id,
                file_name: spec.file_name,
                known_112: sha256 == KNOWN_112_HASHES[spec_index],
                sha256,
                data: data.into_boxed_slice(),
                headers,
            });
        }

        Ok(Self { groups })
    }

    pub fn groups(&self) -> &[WaveGroup] {
        &self.groups
    }

    pub fn group(&self, id: GroupId) -> &WaveGroup {
        self.groups
            .iter()
            .find(|group| group.id == id)
            .expect("all group IDs are installed by WaveBankSet::open")
    }

    pub fn is_known_112(&self) -> bool {
        self.groups.iter().all(WaveGroup::is_known_112)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WaveSegment<'a> {
    group: GroupId,
    index: usize,
    data: &'a [u8],
}

impl<'a> WaveSegment<'a> {
    pub fn group(self) -> GroupId {
        self.group
    }

    pub fn index(self) -> usize {
        self.index
    }

    pub fn bytes(self) -> &'a [u8] {
        self.data
    }

    pub fn scale_exponent(self, sample_address: usize) -> Result<u8, BankError> {
        validate_dpcm_range(sample_address, sample_address.saturating_add(1))?;
        let scale_byte = self.data[sample_address >> 5];
        Ok(if sample_address & 0x10 == 0 {
            scale_byte & 0x0f
        } else {
            scale_byte >> 4
        })
    }

    pub fn decode_fce_dpcm(
        self,
        start: usize,
        length: usize,
        initial_value: i32,
    ) -> Result<Vec<i32>, BankError> {
        let end = start
            .checked_add(length)
            .ok_or(BankError::InvalidDpcmRange {
                start,
                end: usize::MAX,
                minimum: SCALE_TABLE_SIZE,
                maximum: SEGMENT_SIZE,
            })?;
        validate_dpcm_range(start, end)?;

        let mut accumulator = i64::from(initial_value);
        let mut decoded = Vec::with_capacity(length);
        for address in start..end {
            let delta = i64::from(self.data[address] as i8);
            let exponent = self.scale_exponent(address)?;
            accumulator += delta << exponent;
            let value = i32::try_from(accumulator).map_err(|_| BankError::DpcmOverflow {
                sample: address - start,
            })?;
            decoded.push(value);
        }
        Ok(decoded)
    }
}

fn validate_dpcm_range(start: usize, end: usize) -> Result<(), BankError> {
    if start < SCALE_TABLE_SIZE || end > SEGMENT_SIZE || start > end {
        return Err(BankError::InvalidDpcmRange {
            start,
            end,
            minimum: SCALE_TABLE_SIZE,
            maximum: SEGMENT_SIZE,
        });
    }
    Ok(())
}

fn parse_header(segment: &[u8]) -> SegmentHeader {
    SegmentHeader {
        marker: ascii_field(&segment[0x00..0x10]),
        date: ascii_field(&segment[0x10..0x20]),
        config_words: [
            u32::from_le_bytes(segment[0x20..0x24].try_into().unwrap()),
            u32::from_le_bytes(segment[0x24..0x28].try_into().unwrap()),
            u32::from_le_bytes(segment[0x28..0x2c].try_into().unwrap()),
        ],
    }
}

fn ascii_field(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_low_and_high_scale_nibbles() {
        let mut bytes = vec![0_u8; SEGMENT_SIZE];
        bytes[0x400] = 0x31;
        bytes[0x8000] = 2;
        bytes[0x8010] = 2;
        let segment = WaveSegment {
            group: GroupId::Sc88Rev200,
            index: 0,
            data: &bytes,
        };

        assert_eq!(segment.scale_exponent(0x8000).unwrap(), 1);
        assert_eq!(segment.scale_exponent(0x8010).unwrap(), 3);
    }

    #[test]
    fn decodes_cumulative_fce_dpcm() {
        let mut bytes = vec![0_u8; SEGMENT_SIZE];
        bytes[0x400] = 0x21;
        bytes[0x8000] = 2;
        bytes[0x8001] = (-1_i8) as u8;
        bytes[0x8010] = 3;
        let segment = WaveSegment {
            group: GroupId::Sc88Rev200,
            index: 0,
            data: &bytes,
        };

        assert_eq!(segment.decode_fce_dpcm(0x8000, 2, 0).unwrap(), vec![4, 2]);
        assert_eq!(segment.decode_fce_dpcm(0x8010, 1, 0).unwrap(), vec![12]);
    }

    #[test]
    fn rejects_scale_table_as_sample_data() {
        let bytes = vec![0_u8; SEGMENT_SIZE];
        let segment = WaveSegment {
            group: GroupId::Sc8820,
            index: 0,
            data: &bytes,
        };
        assert!(matches!(
            segment.decode_fce_dpcm(0x7fff, 1, 0),
            Err(BankError::InvalidDpcmRange { .. })
        ));
    }

    #[test]
    fn resolves_tone_partial_to_sample_descriptor() {
        let mut samples = vec![0_u8; SAMPLE_SPEC.size];
        let mut wave_maps = vec![0_u8; WAVE_MAP_SPEC.size];
        let mut tones = vec![0_u8; TONE_SPEC.size];
        let mut expected_descriptor = [0_u8; SAMPLE_RECORD_SIZE];
        expected_descriptor[0] = 8;
        expected_descriptor[1..4].copy_from_slice(&[0x07, 0x4e, 0xe0]);
        expected_descriptor[7..10].copy_from_slice(&[0x07, 0xed, 0x23]);
        expected_descriptor[11..14].copy_from_slice(&[0x08, 0x36, 0xde]);

        tones[..12].copy_from_slice(b"Test Tone   ");
        tones[0x16] = 1;
        tones[0x26..0x28].copy_from_slice(&0_u16.to_le_bytes());
        tones[0x28] = 64;
        wave_maps[..12].copy_from_slice(b"Test Map    ");
        wave_maps[0x0c] = 62;
        wave_maps[0x2c..0x2e].copy_from_slice(&7_u16.to_le_bytes());
        wave_maps[0x6c] = 127;
        samples[7 * SAMPLE_RECORD_SIZE..8 * SAMPLE_RECORD_SIZE]
            .copy_from_slice(&expected_descriptor);

        let controls = ControlBankSet {
            samples: samples.into_boxed_slice(),
            wave_maps: wave_maps.into_boxed_slice(),
            tones: tones.into_boxed_slice(),
            interpolation: vec![0_u8; INTERPOLATION_SPEC.size].into_boxed_slice(),
        };
        let result = controls.resolve(0, 60).unwrap();

        assert_eq!(result.tone_name, "Test Tone");
        assert_eq!(result.partials.len(), 1);
        assert_eq!(result.partials[0].wave_map_name, "Test Map");
        assert_eq!(result.partials[0].sample, 7);
        assert_eq!(result.partials[0].descriptor, expected_descriptor);
        assert_eq!(result.partials[0].sample_location.group, GroupId::RomMakeA);
        assert_eq!(result.partials[0].sample_location.group_segment, 0);
        assert_eq!(result.partials[0].sample_location.start, 0x74ee0);
        assert_eq!(result.partials[0].sample_location.loop_start, 0x7ed23);
        assert_eq!(result.partials[0].sample_location.end, 0x836de);
        assert_eq!(result.partials[0].sample_location.root_key, 0);
        assert_eq!(result.partials[0].sample_location.root_pitch_milli, 1024);
    }

    #[test]
    fn rejects_invalid_midi_note_during_resolution() {
        let controls = ControlBankSet {
            samples: vec![0_u8; SAMPLE_SPEC.size].into_boxed_slice(),
            wave_maps: vec![0_u8; WAVE_MAP_SPEC.size].into_boxed_slice(),
            tones: vec![0_u8; TONE_SPEC.size].into_boxed_slice(),
            interpolation: vec![0_u8; INTERPOLATION_SPEC.size].into_boxed_slice(),
        };

        assert!(matches!(
            controls.resolve(0, 128),
            Err(BankError::InvalidNote { note: 128 })
        ));
    }
}
