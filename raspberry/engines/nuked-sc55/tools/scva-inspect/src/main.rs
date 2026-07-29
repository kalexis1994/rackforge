use anyhow::{Context, Result, bail};
use goblin::Object;
use iced_x86::{Decoder, DecoderOptions, FlowControl, Formatter, IntelFormatter};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const MIB: usize = 1024 * 1024;
const SCCORE_1_1_2_SHA256: &str =
    "0635cc2bfced7876694f362f29719bae58e4539d576af9321673f6ffc31f6735";
const SAMPLE_RECORD_SIZE: usize = 0x16;
const SAMPLE_RECORD_COUNT: usize = 4_259;
const WAVE_MAP_RECORD_SIZE: usize = 0x8c;
const WAVE_MAP_RECORD_COUNT: usize = 1_175;
const TONE_RECORD_SIZE: usize = 0x100;
const TONE_RECORD_COUNT: usize = 2_363;

struct ControlBlock {
    name: &'static str,
    offset: usize,
    size: usize,
}

const CONTROL_BLOCKS: &[ControlBlock] = &[
    ControlBlock {
        name: "system.bin",
        offset: 0x189a5d0,
        size: 0x58,
    },
    ControlBlock {
        name: "named-addresses.bin",
        offset: 0x189a630,
        size: 0x4b0,
    },
    ControlBlock {
        name: "sample-descriptors.bin",
        offset: 0x189aae0,
        size: 0x16e04,
    },
    ControlBlock {
        name: "drum-kits.bin",
        offset: 0x18b18f0,
        size: 0x1bc20,
    },
    ControlBlock {
        name: "wave-maps.bin",
        offset: 0x18cd510,
        size: 0x28294,
    },
    ControlBlock {
        name: "tones.bin",
        offset: 0x18f57b0,
        size: 0x93b00,
    },
];

struct WaveGroup {
    name: &'static str,
    segment_count: usize,
    start_offset: usize,
    marker: &'static [u8],
}

struct KnownRom {
    name: &'static str,
    size: usize,
    sha256: &'static str,
}

const KNOWN_MK2_ROMS: &[KnownRom] = &[
    KnownRom {
        name: "rom1.bin",
        size: 0x8000,
        sha256: "8a1eb33c7599b746c0c50283e4349a1bb1773b5c0ec0e9661219bf6c067d2042",
    },
    KnownRom {
        name: "rom2.bin",
        size: 0x80000,
        sha256: "a4c9fd821059054c7e7681d61f49ce6f42ed2fe407a7ec1ba0dfdc9722582ce0",
    },
    KnownRom {
        name: "rom_sm.bin",
        size: 0x1000,
        sha256: "b0b5f865a403f7308b4be8d0ed3ba2ed1c22db881b8a8326769dea222f6431d8",
    },
    KnownRom {
        name: "waverom1.bin",
        size: 0x200000,
        sha256: "c6429e21b9b3a02fbd68ef0b2053668433bee0bccd537a71841bc70b8874243b",
    },
    KnownRom {
        name: "waverom2.bin",
        size: 0x100000,
        sha256: "5b753f6cef4cfc7fcafe1430fecbb94a739b874e55356246a46abe24097ee491",
    },
];

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn entropy(bytes: &[u8]) -> f64 {
    if bytes.is_empty() {
        return 0.0;
    }
    let mut counts = [0_u64; 256];
    for byte in bytes {
        counts[*byte as usize] += 1;
    }
    let length = bytes.len() as f64;
    counts
        .into_iter()
        .filter(|count| *count != 0)
        .map(|count| {
            let probability = count as f64 / length;
            -probability * probability.log2()
        })
        .sum()
}

fn check_candidate(label: &str, offset: usize, bytes: &[u8]) {
    for rom in KNOWN_MK2_ROMS {
        if bytes.len() == rom.size {
            let hash = sha256_hex(bytes);
            let status = if hash == rom.sha256 {
                "EXACT MATCH"
            } else {
                "size only"
            };
            println!(
                "  candidate={label} offset=0x{offset:x} size=0x{:x} \
                 expected={} status={status} sha256={hash}",
                bytes.len(),
                rom.name
            );
        }
    }
}

fn keyword_offsets(bytes: &[u8], keyword: &[u8]) -> Vec<usize> {
    if keyword.is_empty() || bytes.len() < keyword.len() {
        return Vec::new();
    }
    bytes
        .windows(keyword.len())
        .enumerate()
        .filter_map(|(offset, window)| window.eq_ignore_ascii_case(keyword).then_some(offset))
        .take(20)
        .collect()
}

fn all_offsets(bytes: &[u8], marker: &[u8]) -> Vec<usize> {
    bytes
        .windows(marker.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == marker).then_some(offset))
        .collect()
}

fn validate_strided_markers(
    bytes: &[u8],
    marker: &[u8],
    expected_first: usize,
    expected_count: usize,
) -> Result<()> {
    let offsets = all_offsets(bytes, marker);
    let expected: Vec<_> = (0..expected_count)
        .map(|index| expected_first + index * MIB)
        .collect();
    if offsets != expected {
        bail!(
            "unexpected offsets for marker {:?}: found {:?}, expected {:?}",
            String::from_utf8_lossy(marker),
            offsets,
            expected
        );
    }
    Ok(())
}

fn wave_layout(bytes: &[u8]) -> Result<(usize, [WaveGroup; 4])> {
    let first_1994 = all_offsets(bytes, b"1994-12-08")
        .into_iter()
        .next()
        .context("1994 wave marker was not found")?;
    let base = first_1994
        .checked_sub(0x10)
        .context("invalid 1994 marker offset")?;

    let groups = [
        WaveGroup {
            name: "wave_1994_ver200_8mib.bin",
            segment_count: 8,
            start_offset: 0,
            marker: b"ver200",
        },
        WaveGroup {
            name: "wave_1996_rom_make_a_8mib.bin",
            segment_count: 8,
            start_offset: 8 * MIB,
            marker: b"rom_make",
        },
        WaveGroup {
            name: "wave_1996_rom_make_b_4mib.bin",
            segment_count: 4,
            start_offset: 16 * MIB + 0x30,
            marker: b"rom_make",
        },
        WaveGroup {
            name: "wave_1999_sc8820_4mib.bin",
            segment_count: 4,
            start_offset: 20 * MIB + 0x30,
            marker: b"8820_wv0",
        },
    ];

    for group in &groups {
        let marker_start = base
            .checked_add(group.start_offset)
            .context("wave marker offset overflow")?;
        let marker_end = marker_start
            .checked_add(group.marker.len())
            .context("wave marker end overflow")?;
        if bytes.get(marker_start..marker_end) != Some(group.marker) {
            bail!(
                "wave layout validation failed for {} at 0x{marker_start:x}",
                group.name
            );
        }
    }

    validate_strided_markers(bytes, b"1994-12-08", base + 0x10, 8)?;
    let mut expected_1996: Vec<_> = (0..8)
        .map(|index| base + 8 * MIB + 0x10 + index * MIB)
        .collect();
    expected_1996.extend((0..4).map(|index| base + 16 * MIB + 0x40 + index * MIB));
    let found_1996 = all_offsets(bytes, b"1996-06-16");
    if found_1996 != expected_1996 {
        bail!(
            "unexpected offsets for marker \"1996-06-16\": found {:?}, expected {:?}",
            found_1996,
            expected_1996
        );
    }
    validate_strided_markers(bytes, b"1999-08-17", base + 20 * MIB + 0x40, 4)?;

    let end = base
        .checked_add(24 * MIB + 0x30)
        .context("wave layout end overflow")?;
    if end > bytes.len() {
        bail!(
            "wave layout ends at 0x{end:x}, past file size 0x{:x}",
            bytes.len()
        );
    }

    Ok((base, groups))
}

fn print_wave_candidates(bytes: &[u8]) {
    let Ok((base, groups)) = wave_layout(bytes) else {
        return;
    };

    println!("WAVE_LAYOUT base=0x{base:x} span=0x{:x}", 24 * MIB + 0x30);
    for group in groups {
        let group_start = base + group.start_offset;
        println!(
            "  group={} offset=0x{group_start:x} size=0x{:x}",
            group.name,
            group.segment_count * MIB
        );
        for index in 0..group.segment_count {
            let start = group_start + index * MIB;
            let segment = &bytes[start..start + MIB];
            println!(
                "    segment={index:02} offset=0x{start:x} size=0x{MIB:x} sha256={}",
                sha256_hex(segment)
            );
            check_candidate(
                &format!("{}-segment-{index:02}", group.name),
                start,
                segment,
            );
        }
        for index in 0..group.segment_count.saturating_sub(1) {
            let start = group_start + index * MIB;
            check_candidate(
                &format!("{}-pair-{index:02}-{:02}", group.name, index + 1),
                start,
                &bytes[start..start + 2 * MIB],
            );
        }
    }
}

fn ascii_context(bytes: &[u8], offset: usize, length: usize) -> String {
    let start = offset.saturating_sub(32);
    let end = offset
        .saturating_add(length)
        .saturating_add(64)
        .min(bytes.len());
    bytes[start..end]
        .iter()
        .map(|byte| match byte {
            0x20..=0x7e => *byte as char,
            _ => '.',
        })
        .collect()
}

fn inspect(path: &Path) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    println!("FILE {}", path.display());
    println!(
        "size={} sha256={} entropy={:.4}",
        bytes.len(),
        sha256_hex(&bytes),
        entropy(&bytes)
    );

    for keyword in [
        b"SC-55".as_slice(),
        b"SC-88".as_slice(),
        b"SC-8820".as_slice(),
        b"PCM".as_slice(),
        b"WAVE".as_slice(),
        b"ROM".as_slice(),
        b"zlib".as_slice(),
        b"inflate".as_slice(),
        b"uncompress".as_slice(),
        b"Roland XP-GS".as_slice(),
        b"Ver.1.01".as_slice(),
        b"ver200".as_slice(),
        b"Ver200".as_slice(),
        b"1994-12-08".as_slice(),
        b"1999-08-17".as_slice(),
        b"wv0".as_slice(),
        b"GS64 Ver".as_slice(),
    ] {
        let offsets = keyword_offsets(&bytes, keyword);
        if !offsets.is_empty() {
            println!(
                "keyword={} offsets={}",
                String::from_utf8_lossy(keyword),
                offsets
                    .iter()
                    .map(|offset| format!("0x{offset:x}"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            for offset in offsets.iter().take(3) {
                println!(
                    "  context@0x{offset:x}={}",
                    ascii_context(&bytes, *offset, keyword.len())
                );
            }
        }
    }

    match Object::parse(&bytes).context("parsing object format")? {
        Object::PE(pe) => {
            println!(
                "PE is_64={} image_base=0x{:x} entry=0x{:x}",
                pe.is_64, pe.image_base, pe.entry
            );
            println!("SECTIONS");
            for section in &pe.sections {
                let name = section.name().unwrap_or("<invalid>");
                let offset = section.pointer_to_raw_data as usize;
                let size = section.size_of_raw_data as usize;
                let end = offset.saturating_add(size).min(bytes.len());
                let data = bytes.get(offset..end).unwrap_or_default();
                println!(
                    "  name={name:<8} raw=0x{offset:08x} size=0x{size:08x} \
                     virtual=0x{:08x} vsize=0x{:08x} entropy={:.4}",
                    section.virtual_address,
                    section.virtual_size,
                    entropy(data)
                );
                check_candidate(&format!("section:{name}"), offset, data);
                for rom in KNOWN_MK2_ROMS {
                    if data.len() >= rom.size {
                        check_candidate(
                            &format!("section-start:{name}"),
                            offset,
                            &data[..rom.size],
                        );
                        check_candidate(
                            &format!("section-end:{name}"),
                            end - rom.size,
                            &data[data.len() - rom.size..],
                        );
                    }
                }
            }

            let imports: BTreeSet<_> = pe
                .imports
                .iter()
                .map(|import| import.dll.to_string())
                .collect();
            println!(
                "IMPORT_DLLS {}",
                imports.into_iter().collect::<Vec<_>>().join(",")
            );

            println!("EXPORTS count={}", pe.exports.len());
            for export in pe.exports.iter().take(100) {
                println!(
                    "  name={} rva=0x{:x}",
                    export.name.unwrap_or("<ordinal-only>"),
                    export.rva
                );
            }
        }
        other => bail!("unsupported object format: {other:?}"),
    }

    print_wave_candidates(&bytes);
    println!();
    Ok(())
}

fn parse_number(value: &str) -> Result<usize> {
    if let Some(hex) = value.strip_prefix("0x") {
        usize::from_str_radix(hex, 16).with_context(|| format!("invalid hex number {value:?}"))
    } else {
        value
            .parse()
            .with_context(|| format!("invalid decimal number {value:?}"))
    }
}

fn exact_sccore_1_1_2(path: &Path) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let hash = sha256_hex(&bytes);
    if hash != SCCORE_1_1_2_SHA256 {
        bail!(
            "{} has unsupported SHA-256 {hash}; expected Sound Canvas VA 1.1.2 {}",
            path.display(),
            SCCORE_1_1_2_SHA256
        );
    }
    for block in CONTROL_BLOCKS {
        let end = block
            .offset
            .checked_add(block.size)
            .context("control block range overflow")?;
        if end > bytes.len() {
            bail!(
                "{} ends at 0x{end:x}, past file size 0x{:x}",
                block.name,
                bytes.len()
            );
        }
    }
    Ok(bytes)
}

fn fixed_name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches([' ', '\0'])
        .to_owned()
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let value = bytes
        .get(offset..offset + 2)
        .with_context(|| format!("reading u16 at 0x{offset:x}"))?;
    Ok(u16::from_le_bytes(value.try_into().unwrap()))
}

fn resolve_tone(path: &Path, tone_index: usize, note: usize) -> Result<()> {
    if tone_index >= TONE_RECORD_COUNT {
        bail!(
            "tone index {tone_index} is outside 0..{}",
            TONE_RECORD_COUNT - 1
        );
    }
    if note > 127 {
        bail!("MIDI note must be in 0..=127");
    }
    let bytes = exact_sccore_1_1_2(path)?;
    let tone_block = CONTROL_BLOCKS
        .iter()
        .find(|block| block.name == "tones.bin")
        .unwrap();
    let map_block = CONTROL_BLOCKS
        .iter()
        .find(|block| block.name == "wave-maps.bin")
        .unwrap();
    let sample_block = CONTROL_BLOCKS
        .iter()
        .find(|block| block.name == "sample-descriptors.bin")
        .unwrap();
    let tone_offset = tone_block.offset + tone_index * TONE_RECORD_SIZE;
    let tone = &bytes[tone_offset..tone_offset + TONE_RECORD_SIZE];
    let name = fixed_name(&tone[..12]);
    let partial_mask = tone[0x16];

    println!(
        "TONE index={tone_index} name={name:?} note={note} file_offset=0x{tone_offset:x} \
         partial_mask=0x{partial_mask:02x}"
    );
    for partial in 0..2 {
        if partial_mask & (1 << partial) == 0 {
            println!("  partial={} disabled", partial + 1);
            continue;
        }
        let partial_offset = 0x24 + partial * 0x6e;
        let map_index = usize::from(le_u16(tone, partial_offset + 2)?);
        if map_index >= WAVE_MAP_RECORD_COUNT {
            bail!(
                "partial {} references wave map {map_index}, outside 0..{}",
                partial + 1,
                WAVE_MAP_RECORD_COUNT - 1
            );
        }
        let key_bias = usize::from(tone[partial_offset + 4]);
        let mapped_note = note.saturating_add(0x40).saturating_sub(key_bias).min(127);
        let map_offset = map_block.offset + map_index * WAVE_MAP_RECORD_SIZE;
        let map = &bytes[map_offset..map_offset + WAVE_MAP_RECORD_SIZE];
        let map_name = fixed_name(&map[..12]);
        let slot = map[0x0c..0x2c]
            .iter()
            .position(|upper| usize::from(*upper) >= mapped_note)
            .context("wave map has no key range for the requested note")?;
        let upper_note = map[0x0c + slot];
        let sample_index = usize::from(le_u16(map, 0x2c + slot * 2)?);
        if sample_index >= SAMPLE_RECORD_COUNT {
            bail!(
                "partial {} map {} slot {} references sample {sample_index}, outside 0..{}",
                partial + 1,
                map_index,
                slot,
                SAMPLE_RECORD_COUNT - 1
            );
        }
        let map_value = map[0x6c + slot];
        let sample_offset = sample_block.offset + sample_index * SAMPLE_RECORD_SIZE;
        let sample = &bytes[sample_offset..sample_offset + SAMPLE_RECORD_SIZE];
        let descriptor = sample
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join("");
        println!(
            "  partial={} map={} map_name={map_name:?} key_bias={} mapped_note={} \
             slot={} upper_note={} map_value={} sample={} sample_file=0x{sample_offset:x} \
             descriptor={descriptor}",
            partial + 1,
            map_index,
            key_bias,
            mapped_note,
            slot,
            upper_note,
            map_value,
            sample_index
        );
    }
    Ok(())
}

fn file_offset_to_rva(pe: &goblin::pe::PE<'_>, offset: usize) -> Option<u64> {
    pe.sections.iter().find_map(|section| {
        let start = section.pointer_to_raw_data as usize;
        let end = start.checked_add(section.size_of_raw_data as usize)?;
        (start..end)
            .contains(&offset)
            .then_some(u64::from(section.virtual_address) + u64::try_from(offset - start).ok()?)
    })
}

fn rva_to_file_offset(pe: &goblin::pe::PE<'_>, rva: u64) -> Option<usize> {
    pe.sections.iter().find_map(|section| {
        let start = u64::from(section.virtual_address);
        let size = u64::from(section.virtual_size.max(section.size_of_raw_data));
        let end = start.checked_add(size)?;
        (start..end)
            .contains(&rva)
            .then_some(section.pointer_to_raw_data as usize + usize::try_from(rva - start).ok()?)
    })
}

fn xrefs(path: &Path, target_offset: usize, target_length: usize) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let Object::PE(pe) = Object::parse(&bytes).context("parsing PE file")? else {
        bail!("xrefs mode supports PE files only");
    };
    if !pe.is_64 {
        bail!("xrefs mode currently supports PE32+ x86-64 only");
    }
    let target_end = target_offset
        .checked_add(target_length)
        .context("target range overflow")?;
    let target_rva = file_offset_to_rva(&pe, target_offset)
        .with_context(|| format!("file offset 0x{target_offset:x} is outside PE sections"))?;
    let target_end_rva = file_offset_to_rva(&pe, target_end.saturating_sub(1))
        .with_context(|| format!("target end 0x{target_end:x} is outside PE sections"))?
        + 1;

    let text = pe
        .sections
        .iter()
        .find(|section| section.name().ok() == Some(".text"))
        .context("PE has no .text section")?;
    let text_offset = text.pointer_to_raw_data as usize;
    let text_end = text_offset
        .checked_add(text.size_of_raw_data as usize)
        .context(".text range overflow")?
        .min(bytes.len());
    let text_bytes = &bytes[text_offset..text_end];
    let text_ip = pe.image_base + u64::from(text.virtual_address);
    let mut decoder = Decoder::with_ip(64, text_bytes, text_ip, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    formatter.options_mut().set_rip_relative_addresses(false);
    let mut formatted = String::new();
    let mut count = 0_usize;

    println!(
        "XREFS file={} target_file=0x{target_offset:x}..0x{target_end:x} \
         target_rva=0x{target_rva:x}..0x{target_end_rva:x}",
        path.display()
    );
    while decoder.can_decode() {
        let instruction = decoder.decode();
        if !instruction.is_ip_rel_memory_operand() {
            continue;
        }
        let address = instruction.ip_rel_memory_address();
        let Some(rva) = address.checked_sub(pe.image_base) else {
            continue;
        };
        if !(target_rva..target_end_rva).contains(&rva) {
            continue;
        }

        formatted.clear();
        formatter.format(&instruction, &mut formatted);
        let instruction_rva = instruction.ip() - pe.image_base;
        let instruction_offset = rva_to_file_offset(&pe, instruction_rva).unwrap_or(usize::MAX);
        let referenced_offset = rva_to_file_offset(&pe, rva).unwrap_or(usize::MAX);
        println!(
            "  code_rva=0x{instruction_rva:x} code_file=0x{instruction_offset:x} \
             target_rva=0x{rva:x} target_file=0x{referenced_offset:x} {formatted}"
        );
        count += 1;
    }
    println!("XREF_COUNT {count}");
    Ok(())
}

fn xrefs_rva(path: &Path, target_rva: usize, target_length: usize) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let Object::PE(pe) = Object::parse(&bytes).context("parsing PE file")? else {
        bail!("xrefs-rva mode supports PE files only");
    };
    if !pe.is_64 {
        bail!("xrefs-rva mode currently supports PE32+ x86-64 only");
    }
    let target_rva = target_rva as u64;
    let target_end_rva = target_rva
        .checked_add(target_length as u64)
        .context("target RVA range overflow")?;

    let text = pe
        .sections
        .iter()
        .find(|section| section.name().ok() == Some(".text"))
        .context("PE has no .text section")?;
    let text_offset = text.pointer_to_raw_data as usize;
    let text_end = text_offset
        .checked_add(text.size_of_raw_data as usize)
        .context(".text range overflow")?
        .min(bytes.len());
    let text_bytes = &bytes[text_offset..text_end];
    let text_ip = pe.image_base + u64::from(text.virtual_address);
    let mut decoder = Decoder::with_ip(64, text_bytes, text_ip, DecoderOptions::NONE);
    let mut formatter = IntelFormatter::new();
    formatter.options_mut().set_rip_relative_addresses(false);
    let mut formatted = String::new();
    let mut count = 0_usize;

    println!(
        "XREFS_RVA file={} target_rva=0x{target_rva:x}..0x{target_end_rva:x}",
        path.display()
    );
    while decoder.can_decode() {
        let instruction = decoder.decode();
        if !instruction.is_ip_rel_memory_operand() {
            continue;
        }
        let address = instruction.ip_rel_memory_address();
        let Some(rva) = address.checked_sub(pe.image_base) else {
            continue;
        };
        if !(target_rva..target_end_rva).contains(&rva) {
            continue;
        }

        formatted.clear();
        formatter.format(&instruction, &mut formatted);
        let instruction_rva = instruction.ip() - pe.image_base;
        let instruction_offset = rva_to_file_offset(&pe, instruction_rva).unwrap_or(usize::MAX);
        println!(
            "  code_rva=0x{instruction_rva:x} code_file=0x{instruction_offset:x} \
             target_rva=0x{rva:x} {formatted}"
        );
        count += 1;
    }
    println!("XREF_COUNT {count}");
    Ok(())
}

fn callers_rva(path: &Path, target_rva: usize) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let Object::PE(pe) = Object::parse(&bytes).context("parsing PE file")? else {
        bail!("callers-rva mode supports PE files only");
    };
    if !pe.is_64 {
        bail!("callers-rva mode currently supports PE32+ x86-64 only");
    }

    let text = pe
        .sections
        .iter()
        .find(|section| section.name().ok() == Some(".text"))
        .context("PE has no .text section")?;
    let text_offset = text.pointer_to_raw_data as usize;
    let text_end = text_offset
        .checked_add(text.size_of_raw_data as usize)
        .context(".text range overflow")?
        .min(bytes.len());
    let text_ip = pe.image_base + u64::from(text.virtual_address);
    let mut decoder = Decoder::with_ip(
        64,
        &bytes[text_offset..text_end],
        text_ip,
        DecoderOptions::NONE,
    );
    let mut formatter = IntelFormatter::new();
    let mut formatted = String::new();
    let target_va = pe.image_base + target_rva as u64;
    let mut count = 0_usize;

    println!(
        "CALLERS_RVA file={} target_rva=0x{target_rva:x}",
        path.display()
    );
    while decoder.can_decode() {
        let instruction = decoder.decode();
        if !matches!(
            instruction.flow_control(),
            FlowControl::Call | FlowControl::UnconditionalBranch
        ) || instruction.near_branch_target() != target_va
        {
            continue;
        }
        formatted.clear();
        formatter.format(&instruction, &mut formatted);
        let instruction_rva = instruction.ip() - pe.image_base;
        println!(
            "  code_rva=0x{instruction_rva:x} code_file=0x{:x} {formatted}",
            rva_to_file_offset(&pe, instruction_rva).unwrap_or(usize::MAX)
        );
        count += 1;
    }
    println!("CALLER_COUNT {count}");
    Ok(())
}

fn pointers_to(path: &Path, target_offset: usize, target_length: usize) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let Object::PE(pe) = Object::parse(&bytes).context("parsing PE file")? else {
        bail!("pointers mode supports PE files only");
    };
    let target_end = target_offset
        .checked_add(target_length)
        .context("target range overflow")?;
    let target_rva = file_offset_to_rva(&pe, target_offset)
        .with_context(|| format!("file offset 0x{target_offset:x} is outside PE sections"))?;
    let target_end_rva = file_offset_to_rva(&pe, target_end.saturating_sub(1))
        .with_context(|| format!("target end 0x{target_end:x} is outside PE sections"))?
        + 1;
    let target_va = pe.image_base + target_rva;
    let target_end_va = pe.image_base + target_end_rva;
    let mut va_count = 0_usize;
    let mut rva_count = 0_usize;

    println!(
        "POINTERS file={} target_file=0x{target_offset:x}..0x{target_end:x} \
         target_rva=0x{target_rva:x}..0x{target_end_rva:x}",
        path.display()
    );
    for section in &pe.sections {
        let section_name = section.name().unwrap_or("<invalid>");
        if section_name == ".text" {
            continue;
        }
        let start = section.pointer_to_raw_data as usize;
        let end = start
            .saturating_add(section.size_of_raw_data as usize)
            .min(bytes.len());
        let data = &bytes[start..end];

        for (relative, window) in data.windows(8).enumerate() {
            let value = u64::from_le_bytes(window.try_into().unwrap());
            if (target_va..target_end_va).contains(&value) {
                let source = start + relative;
                println!(
                    "  kind=va64 section={section_name} source_file=0x{source:x} \
                     target_file=0x{:x}",
                    rva_to_file_offset(&pe, value - pe.image_base).unwrap_or(usize::MAX)
                );
                va_count += 1;
            }
        }
        for (relative, window) in data.windows(4).enumerate() {
            let value = u64::from(u32::from_le_bytes(window.try_into().unwrap()));
            if (target_rva..target_end_rva).contains(&value) {
                let source = start + relative;
                println!(
                    "  kind=rva32 section={section_name} source_file=0x{source:x} \
                     target_file=0x{:x}",
                    rva_to_file_offset(&pe, value).unwrap_or(usize::MAX)
                );
                rva_count += 1;
            }
        }
    }
    println!("POINTER_COUNT va64={va_count} rva32={rva_count}");
    Ok(())
}

fn disassemble(path: &Path, start_rva: usize, length: usize) -> Result<()> {
    let bytes = fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let Object::PE(pe) = Object::parse(&bytes).context("parsing PE file")? else {
        bail!("disassembly mode supports PE files only");
    };
    if !pe.is_64 {
        bail!("disassembly mode currently supports PE32+ x86-64 only");
    }
    let start_offset = rva_to_file_offset(&pe, start_rva as u64)
        .with_context(|| format!("RVA 0x{start_rva:x} is outside PE sections"))?;
    let end_offset = start_offset
        .checked_add(length)
        .context("disassembly range overflow")?
        .min(bytes.len());
    let mut decoder = Decoder::with_ip(
        64,
        &bytes[start_offset..end_offset],
        pe.image_base + start_rva as u64,
        DecoderOptions::NONE,
    );
    let mut formatter = IntelFormatter::new();
    formatter.options_mut().set_rip_relative_addresses(false);
    let mut formatted = String::new();
    println!(
        "DISASSEMBLY file={} rva=0x{start_rva:x} file_offset=0x{start_offset:x} \
         length=0x{length:x}",
        path.display()
    );
    while decoder.can_decode() {
        let instruction = decoder.decode();
        formatted.clear();
        formatter.format(&instruction, &mut formatted);
        println!(
            "  rva=0x{:x} file=0x{:x} {formatted}",
            instruction.ip() - pe.image_base,
            start_offset + decoder.position() - instruction.len()
        );
    }
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .context("output path has no file name")?
        .to_string_lossy();
    let temporary = path.with_file_name(format!(".{file_name}.tmp"));
    fs::write(&temporary, bytes)
        .with_context(|| format!("writing temporary file {}", temporary.display()))?;
    fs::rename(&temporary, path)
        .with_context(|| format!("moving temporary file into {}", path.display()))?;
    Ok(())
}

fn dump_waves(source: &Path, output: &Path) -> Result<()> {
    let bytes = fs::read(source).with_context(|| format!("reading {}", source.display()))?;
    let (base, groups) = wave_layout(&bytes)?;

    if output.exists() {
        let mut entries =
            fs::read_dir(output).with_context(|| format!("reading {}", output.display()))?;
        if entries.next().is_some() {
            bail!(
                "refusing to write into non-empty directory {}",
                output.display()
            );
        }
    } else {
        fs::create_dir_all(output).with_context(|| format!("creating {}", output.display()))?;
    }

    let mut manifest = String::new();
    writeln!(&mut manifest, "source={}", source.display())?;
    writeln!(&mut manifest, "source_size={}", bytes.len())?;
    writeln!(&mut manifest, "source_sha256={}", sha256_hex(&bytes))?;
    writeln!(&mut manifest, "wave_base=0x{base:x}")?;
    writeln!(&mut manifest, "format=name\\toffset\\tsize\\tsha256")?;

    let full_region_size = 24 * MIB + 0x30;
    let full_region_name = "wave_region_complete.bin";
    let full_region = &bytes[base..base + full_region_size];
    atomic_write(&output.join(full_region_name), full_region)?;
    writeln!(
        &mut manifest,
        "{full_region_name}\t0x{base:x}\t0x{full_region_size:x}\t{}",
        sha256_hex(full_region)
    )?;
    println!(
        "DUMPED {} offset=0x{base:x} size=0x{full_region_size:x} sha256={}",
        output.join(full_region_name).display(),
        sha256_hex(full_region)
    );

    for group in groups {
        let group_base = base + group.start_offset;
        let size = group.segment_count * MIB;
        let data = &bytes[group_base..group_base + size];
        let path = output.join(group.name);
        atomic_write(&path, data)?;
        writeln!(
            &mut manifest,
            "{}\t0x{:x}\t0x{:x}\t{}",
            group.name,
            group_base,
            size,
            sha256_hex(data)
        )?;
        println!(
            "DUMPED {} offset=0x{group_base:x} size=0x{size:x} sha256={}",
            path.display(),
            sha256_hex(data)
        );
    }

    let manifest_path = output.join("manifest.txt");
    atomic_write(&manifest_path, manifest.as_bytes())?;
    println!("MANIFEST {}", manifest_path.display());
    Ok(())
}

fn prepare_empty_output(output: &Path) -> Result<()> {
    if output.exists() {
        let mut entries =
            fs::read_dir(output).with_context(|| format!("reading {}", output.display()))?;
        if entries.next().is_some() {
            bail!(
                "refusing to write into non-empty directory {}",
                output.display()
            );
        }
    } else {
        fs::create_dir_all(output).with_context(|| format!("creating {}", output.display()))?;
    }
    Ok(())
}

fn dump_control(source: &Path, output: &Path) -> Result<()> {
    let bytes = exact_sccore_1_1_2(source)?;
    prepare_empty_output(output)?;

    let mut manifest = String::new();
    writeln!(&mut manifest, "format=artupy-scva-control-v1")?;
    writeln!(&mut manifest, "source={}", source.display())?;
    writeln!(&mut manifest, "source_size={}", bytes.len())?;
    writeln!(&mut manifest, "source_sha256={}", sha256_hex(&bytes))?;
    writeln!(&mut manifest, "sample_record_size=0x{SAMPLE_RECORD_SIZE:x}")?;
    writeln!(&mut manifest, "sample_record_count={SAMPLE_RECORD_COUNT}")?;
    writeln!(
        &mut manifest,
        "wave_map_record_size=0x{WAVE_MAP_RECORD_SIZE:x}"
    )?;
    writeln!(
        &mut manifest,
        "wave_map_record_count={WAVE_MAP_RECORD_COUNT}"
    )?;
    writeln!(&mut manifest, "tone_record_size=0x{TONE_RECORD_SIZE:x}")?;
    writeln!(&mut manifest, "tone_record_count={TONE_RECORD_COUNT}")?;
    writeln!(&mut manifest, "files=name\\toffset\\tsize\\tsha256")?;

    for block in CONTROL_BLOCKS {
        let data = &bytes[block.offset..block.offset + block.size];
        let path = output.join(block.name);
        atomic_write(&path, data)?;
        let hash = sha256_hex(data);
        writeln!(
            &mut manifest,
            "{}\t0x{:x}\t0x{:x}\t{hash}",
            block.name, block.offset, block.size
        )?;
        println!(
            "DUMPED {} offset=0x{:x} size=0x{:x} sha256={hash}",
            path.display(),
            block.offset,
            block.size
        );
    }
    let manifest_path = output.join("manifest.txt");
    atomic_write(&manifest_path, manifest.as_bytes())?;
    println!("MANIFEST {}", manifest_path.display());
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 4 && args[0].to_string_lossy() == "--disasm-rva" {
        return disassemble(
            Path::new(&args[3]),
            parse_number(&args[1].to_string_lossy())?,
            parse_number(&args[2].to_string_lossy())?,
        );
    }
    if args.len() == 4 && args[0].to_string_lossy() == "--pointers-to" {
        return pointers_to(
            Path::new(&args[3]),
            parse_number(&args[1].to_string_lossy())?,
            parse_number(&args[2].to_string_lossy())?,
        );
    }
    if args.len() == 4 && args[0].to_string_lossy() == "--xrefs" {
        return xrefs(
            Path::new(&args[3]),
            parse_number(&args[1].to_string_lossy())?,
            parse_number(&args[2].to_string_lossy())?,
        );
    }
    if args.len() == 4 && args[0].to_string_lossy() == "--xrefs-rva" {
        return xrefs_rva(
            Path::new(&args[3]),
            parse_number(&args[1].to_string_lossy())?,
            parse_number(&args[2].to_string_lossy())?,
        );
    }
    if args.len() == 3 && args[0].to_string_lossy() == "--callers-rva" {
        return callers_rva(
            Path::new(&args[2]),
            parse_number(&args[1].to_string_lossy())?,
        );
    }
    if args.len() == 3 && args[0].to_string_lossy() == "--dump-waves" {
        return dump_waves(Path::new(&args[2]), Path::new(&args[1]));
    }
    if args.len() == 3 && args[0].to_string_lossy() == "--dump-control" {
        return dump_control(Path::new(&args[2]), Path::new(&args[1]));
    }
    if args.len() == 4 && args[0].to_string_lossy() == "--resolve-tone" {
        return resolve_tone(
            Path::new(&args[3]),
            parse_number(&args[1].to_string_lossy())?,
            parse_number(&args[2].to_string_lossy())?,
        );
    }
    if args.is_empty() {
        bail!(
            "usage:\n  scva-inspect FILE [FILE...]\n  \
             scva-inspect --dump-waves OUTPUT_DIRECTORY SCCore.dll\n  \
             scva-inspect --dump-control OUTPUT_DIRECTORY SCCore.dll\n  \
             scva-inspect --resolve-tone TONE_INDEX MIDI_NOTE SCCore.dll\n  \
             scva-inspect --xrefs FILE_OFFSET LENGTH SCCore.dll\n  \
             scva-inspect --xrefs-rva RVA LENGTH SCCore.dll\n  \
             scva-inspect --callers-rva RVA SCCore.dll\n  \
             scva-inspect --pointers-to FILE_OFFSET LENGTH SCCore.dll\n  \
             scva-inspect --disasm-rva RVA LENGTH SCCore.dll"
        );
    }
    if args
        .first()
        .is_some_and(|arg| arg.to_string_lossy().starts_with('-'))
    {
        bail!("unknown option: {}", args[0].to_string_lossy());
    }
    for path in args {
        inspect(Path::new(&path))?;
    }
    Ok(())
}
