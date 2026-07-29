use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

pub const SEGMENT_SIZE: usize = 1024 * 1024;
pub const SCALE_TABLE_SIZE: usize = SEGMENT_SIZE / 32;

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
}
