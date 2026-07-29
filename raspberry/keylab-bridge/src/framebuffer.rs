const SYSEX_PREFIX: &[u8] = &[0xF0, 0x00, 0x20, 0x6B, 0x7F, 0x42];
const MAGIC: &[u8] = &[0x7D, b'A', b'P', 0x01];
const OP_FRAME_CHUNK: u8 = 0x01;
const OP_PRESENT: u8 = 0x02;
pub const FRAMEBUFFER_LEN: usize = 1024;
const MAX_CHUNK_LEN: usize = 40;

pub fn solid_upload(on: bool) -> Vec<Vec<u8>> {
    let value = if on { 0xFF } else { 0x00 };
    upload_messages(&[value; FRAMEBUFFER_LEN])
}

fn upload_messages(frame: &[u8; FRAMEBUFFER_LEN]) -> Vec<Vec<u8>> {
    let mut messages = Vec::with_capacity(FRAMEBUFFER_LEN.div_ceil(MAX_CHUNK_LEN) + 1);
    for offset in (0..FRAMEBUFFER_LEN).step_by(MAX_CHUNK_LEN) {
        let end = (offset + MAX_CHUNK_LEN).min(FRAMEBUFFER_LEN);
        messages.push(frame_chunk(offset, &frame[offset..end]));
    }
    messages.push(present(frame));
    messages
}

fn frame_chunk(offset: usize, raw: &[u8]) -> Vec<u8> {
    debug_assert!(!raw.is_empty() && raw.len() <= MAX_CHUNK_LEN);
    debug_assert!(offset + raw.len() <= FRAMEBUFFER_LEN);

    let mut payload = Vec::with_capacity(9 + raw.len() * 2);
    payload.extend_from_slice(MAGIC);
    payload.extend_from_slice(&[
        OP_FRAME_CHUNK,
        (offset & 0x7F) as u8,
        ((offset >> 7) & 0x7F) as u8,
        raw.len() as u8,
    ]);
    for value in raw {
        payload.extend_from_slice(&[value & 0x0F, value >> 4]);
    }
    let checksum = payload
        .iter()
        .fold(0_u8, |accumulator, value| accumulator ^ value);
    payload.push(checksum & 0x7F);
    sysex(&payload)
}

fn present(frame: &[u8; FRAMEBUFFER_LEN]) -> Vec<u8> {
    let crc = crc16_ccitt_false(frame);
    sysex(&[
        MAGIC[0],
        MAGIC[1],
        MAGIC[2],
        MAGIC[3],
        OP_PRESENT,
        (crc & 0x7F) as u8,
        ((crc >> 7) & 0x7F) as u8,
        ((crc >> 14) & 0x03) as u8,
    ])
}

fn sysex(payload: &[u8]) -> Vec<u8> {
    debug_assert!(payload.iter().all(|value| *value <= 0x7F));
    let mut message = Vec::with_capacity(SYSEX_PREFIX.len() + payload.len() + 1);
    message.extend_from_slice(SYSEX_PREFIX);
    message.extend_from_slice(payload);
    message.push(0xF7);
    message
}

fn crc16_ccitt_false(data: &[u8]) -> u16 {
    let mut crc = 0xFFFF_u16;
    for value in data {
        crc ^= (*value as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_upload_is_bounded_and_midi_safe() {
        let messages = solid_upload(true);
        assert_eq!(messages.len(), 27);
        assert!(messages.iter().all(|message| {
            message.first() == Some(&0xF0)
                && message.last() == Some(&0xF7)
                && message[1..message.len() - 1]
                    .iter()
                    .all(|value| *value <= 0x7F)
        }));
        assert!(messages.iter().all(|message| message.len() <= 96));
    }

    #[test]
    fn crc_matches_the_host_reference_vectors() {
        assert_eq!(crc16_ccitt_false(&[0; FRAMEBUFFER_LEN]), 0xB76F);
        assert_eq!(crc16_ccitt_false(&[0xFF; FRAMEBUFFER_LEN]), 0x77EB);
    }
}
