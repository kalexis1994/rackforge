use anyhow::{Context, Result, bail};
use goblin::Object;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

const MIB: usize = 1024 * 1024;

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

fn main() {
    if let Err(error) = run() {
        eprintln!("ERROR: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() == 3 && args[0].to_string_lossy() == "--dump-waves" {
        return dump_waves(Path::new(&args[2]), Path::new(&args[1]));
    }
    if args.is_empty() {
        bail!(
            "usage:\n  scva-inspect FILE [FILE...]\n  \
             scva-inspect --dump-waves OUTPUT_DIRECTORY SCCore.dll"
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
