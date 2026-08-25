//! Offline inspector for Arturia KeyLab Essential mk3 firmware payloads.
//!
//! This program is deliberately read-only. It does not open a MIDI/USB device,
//! implement DFU, or contain any write/erase command.

use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

const HEADER_LEN: usize = 0x40;
const INTERNAL_FLASH_START: u32 = 0x0800_0000;
const INTERNAL_FLASH_END: u32 = 0x0804_0000;
const SRAM_START: u32 = 0x2000_0000;
const SRAM_END: u32 = 0x2002_4000;

#[derive(Debug, PartialEq)]
struct Header {
    vendor_id: u16,
    product_id: u16,
    image_index: u16,
    payload_len: u32,
    unknown_0a: u16,
    app_base: u32,
    image_end_inclusive: u32,
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let raw = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| format!("campo u16 truncado en 0x{offset:X}"))?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| format!("campo u32 truncado en 0x{offset:X}"))?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn parse_header(bytes: &[u8]) -> Result<Header, String> {
    if bytes.len() < HEADER_LEN {
        return Err(format!(
            "archivo demasiado pequeño: {} bytes; se esperaban al menos {HEADER_LEN}",
            bytes.len()
        ));
    }
    Ok(Header {
        vendor_id: le_u16(bytes, 0x00)?,
        product_id: le_u16(bytes, 0x02)?,
        image_index: le_u16(bytes, 0x04)?,
        payload_len: le_u32(bytes, 0x06)?,
        unknown_0a: le_u16(bytes, 0x0A)?,
        app_base: le_u32(bytes, 0x0C)?,
        image_end_inclusive: le_u32(bytes, 0x10)?,
    })
}

fn count_aligned_words(bytes: &[u8], needle: u32) -> usize {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|word| u32::from_le_bytes(**word) == needle)
        .count()
}

fn kib(bytes: u32) -> f64 {
    f64::from(bytes) / 1024.0
}

fn mib(bytes: u32) -> f64 {
    f64::from(bytes) / (1024.0 * 1024.0)
}

fn inspect(path: &Path) -> Result<(), Box<dyn Error>> {
    let bytes = fs::read(path)?;
    let header = parse_header(&bytes)?;
    let actual_payload_len = bytes.len() - HEADER_LEN;
    let initial_sp = le_u32(&bytes, HEADER_LEN)?;
    let reset_handler = le_u32(&bytes, HEADER_LEN + 4)?;

    println!("Archivo: {}", path.display());
    println!("Tamaño total: {} bytes", bytes.len());
    println!();
    println!("Cabecera Arturia (0x40 bytes)");
    println!("  VID:              0x{:04X}", header.vendor_id);
    println!("  PID:              0x{:04X}", header.product_id);
    println!("  Índice de imagen: {}", header.image_index);
    println!(
        "  Payload declarado: {} bytes ({:.2} KiB)",
        header.payload_len,
        kib(header.payload_len)
    );
    println!("  Campo 0x0A:       0x{:04X}", header.unknown_0a);
    println!("  Base de app:      0x{:08X}", header.app_base);
    println!("  Fin declarado:    0x{:08X}", header.image_end_inclusive);
    println!();
    println!("Vector Cortex-M");
    println!("  SP inicial:        0x{initial_sp:08X}");
    println!("  Reset handler:     0x{reset_handler:08X}");

    let payload_matches = actual_payload_len == header.payload_len as usize;
    let sp_valid = (SRAM_START..=SRAM_END).contains(&initial_sp);
    let reset_address = reset_handler & !1;
    let reset_valid = (header.app_base..INTERNAL_FLASH_END).contains(&reset_address);

    println!();
    println!("Validaciones");
    println!(
        "  Longitud payload: {} (real: {} bytes)",
        yes_no(payload_matches),
        actual_payload_len
    );
    println!("  SP dentro de SRAM: {}", yes_no(sp_valid));
    println!(
        "  Reset en app/Thumb: {}",
        yes_no(reset_valid && reset_handler & 1 == 1)
    );

    let reserved = header.app_base.saturating_sub(INTERNAL_FLASH_START);
    let slot = INTERNAL_FLASH_END.saturating_sub(header.app_base);
    let nominal_free = slot.saturating_sub(header.payload_len);
    let sram = SRAM_END - SRAM_START;

    println!();
    println!("Mapa inferido (N32G455 de esta unidad)");
    println!(
        "  Flash interna: 0x{INTERNAL_FLASH_START:08X}..0x{INTERNAL_FLASH_END:08X} = {:.0} KiB ({:.6} MiB)",
        kib(INTERNAL_FLASH_END - INTERNAL_FLASH_START),
        mib(INTERNAL_FLASH_END - INTERNAL_FLASH_START)
    );
    println!(
        "  Reservado/boot: 0x{INTERNAL_FLASH_START:08X}..0x{:08X} = {:.0} KiB",
        header.app_base,
        kib(reserved)
    );
    println!(
        "  Slot de app:    0x{:08X}..0x{INTERNAL_FLASH_END:08X} = {:.0} KiB",
        header.app_base,
        kib(slot)
    );
    println!(
        "  Margen nominal: {} bytes ({:.2} KiB)",
        nominal_free,
        kib(nominal_free)
    );
    println!(
        "  SRAM:           0x{SRAM_START:08X}..0x{SRAM_END:08X} = {:.0} KiB ({:.6} MiB)",
        kib(sram),
        mib(sram)
    );

    println!();
    println!("Referencias alineadas a periféricos");
    for (name, address) in [
        ("RCC", 0x4002_1000),
        ("FLASH", 0x4002_2000),
        ("USB FS", 0x4000_5C00),
        ("SPI1", 0x4001_3000),
        ("QSPI regs", 0xA000_1000),
        ("QSPI XIP", 0x9000_0000),
    ] {
        println!(
            "  {name:9} 0x{address:08X}: {}",
            count_aligned_words(&bytes[HEADER_LEN..], address)
        );
    }

    if !(payload_matches && sp_valid && reset_valid) {
        return Err("una o más validaciones estructurales fallaron".into());
    }
    Ok(())
}

fn yes_no(value: bool) -> &'static str {
    if value { "sí" } else { "NO" }
}

fn main() {
    let mut args = env::args_os();
    let program = args.next().unwrap_or_default();
    let Some(path) = args.next() else {
        eprintln!(
            "Uso: {} RUTA_AL_BINARIO\nSolo lee el archivo; nunca abre ni modifica el teclado.",
            Path::new(&program).display()
        );
        std::process::exit(2);
    };
    if args.next().is_some() {
        eprintln!("ERROR: se esperaba una sola ruta");
        std::process::exit(2);
    }
    if let Err(error) = inspect(Path::new(&path)) {
        eprintln!("ERROR: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_header_shape() {
        let mut bytes = vec![0_u8; HEADER_LEN];
        bytes[0x00..0x02].copy_from_slice(&0x1C75_u16.to_le_bytes());
        bytes[0x02..0x04].copy_from_slice(&0x028C_u16.to_le_bytes());
        bytes[0x04..0x06].copy_from_slice(&0_u16.to_le_bytes());
        bytes[0x06..0x0A].copy_from_slice(&193_348_u32.to_le_bytes());
        bytes[0x0A..0x0C].copy_from_slice(&0x01D9_u16.to_le_bytes());
        bytes[0x0C..0x10].copy_from_slice(&0x0800_7800_u32.to_le_bytes());
        bytes[0x10..0x14].copy_from_slice(&0x0803_FFF7_u32.to_le_bytes());

        assert_eq!(
            parse_header(&bytes).unwrap(),
            Header {
                vendor_id: 0x1C75,
                product_id: 0x028C,
                image_index: 0,
                payload_len: 193_348,
                unknown_0a: 0x01D9,
                app_base: 0x0800_7800,
                image_end_inclusive: 0x0803_FFF7,
            }
        );
    }

    #[test]
    fn counts_only_aligned_words() {
        let bytes = [
            0x00, 0x10, 0x00, 0xA0, // aligned 0xA0001000
            0xFF, 0x00, 0x10, 0x00, // same bytes, deliberately unaligned
            0xA0, 0xFF, 0xFF, 0xFF,
        ];
        assert_eq!(count_aligned_words(&bytes, 0xA000_1000), 1);
    }
}
