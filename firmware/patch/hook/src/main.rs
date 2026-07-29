#![no_std]
#![no_main]

use core::arch::global_asm;
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};

const MESSAGE_PAYLOAD_OFFSET: usize = 13;
const MESSAGE_LENGTH_OFFSET: usize = 7;
const FRAMEBUFFER: usize = 0x2000_8514;
const FRAMEBUFFER_LEN: usize = 1024;
const LCD_BUS: usize = 0x2000_2130;
const LCD_FLUSH_THUMB: usize = 0x0800_95B9;

const MAGIC_0: u8 = 0x7D;
const MAGIC_1: u8 = b'A';
const MAGIC_2: u8 = b'P';
const PROTOCOL_VERSION: u8 = 1;
const OP_FRAME_CHUNK: u8 = 1;
const OP_PRESENT: u8 = 2;
const MAX_CHUNK_LEN: usize = 40;

global_asm!(
    r#"
    .syntax unified
    .thumb

    .section .hook_site, "ax", %progbits
    .global rackforge_hook_site
    .type rackforge_hook_site, %function
    .thumb_func
rackforge_hook_site:
    b.w rackforge_hook_entry
    .size rackforge_hook_site, . - rackforge_hook_site

    .section .text.entry, "ax", %progbits
    .global rackforge_hook_entry
    .type rackforge_hook_entry, %function
    .thumb_func
rackforge_hook_entry:
    /*
     * The raw callback receives the stripped Arturia payload at message+13.
     * Keep non-RackForge messages completely outside Rust so the official
     * protocol follows a minimal, register-transparent delegation path.
     */
    push {r0-r4, lr}
    cbz r1, 2f
    ldrb r2, [r1, #7]
    cmp r2, #6
    blo 2f
    ldrb r2, [r1, #13]
    cmp r2, #0x7d
    bne 2f
    ldrb r2, [r1, #14]
    cmp r2, #0x41
    bne 2f
    ldrb r2, [r1, #15]
    cmp r2, #0x50
    bne 2f
    ldrb r2, [r1, #16]
    cmp r2, #1
    bne 2f
    bl rackforge_try_handle
    pop {r0-r4, lr}
    bx lr
2:
    pop {r0-r4, lr}
    /* Replay the raw callback wrapper displaced at 0x0800B802. */
    push {r4, lr}
    ldr r12, =0x0800B7ED
    blx r12
    ldr r12, =0x0800B809
    bx r12
    .size rackforge_hook_entry, . - rackforge_hook_entry
"#,
    options(raw)
);

#[unsafe(no_mangle)]
unsafe extern "C" fn rackforge_try_handle(_context: *mut u8, message: *mut u8) {
    if message.is_null() {
        return;
    }

    let message_len = unsafe { read_volatile(message.add(MESSAGE_LENGTH_OFFSET)) as usize };
    if message_len < 6 {
        return;
    }

    let payload = unsafe { message.add(MESSAGE_PAYLOAD_OFFSET) };
    let is_rackforge = unsafe {
        read_volatile(payload) == MAGIC_0
            && read_volatile(payload.add(1)) == MAGIC_1
            && read_volatile(payload.add(2)) == MAGIC_2
            && read_volatile(payload.add(3)) == PROTOCOL_VERSION
    };
    if !is_rackforge {
        return;
    }

    let operation = unsafe { read_volatile(payload.add(4)) };
    match operation {
        OP_FRAME_CHUNK => unsafe { handle_frame_chunk(payload, message_len) },
        OP_PRESENT => unsafe { handle_present(payload, message_len) },
        _ => {}
    }
}

unsafe fn handle_frame_chunk(payload: *mut u8, message_len: usize) {
    if message_len < 10 {
        return;
    }

    let offset = unsafe {
        read_volatile(payload.add(5)) as usize | ((read_volatile(payload.add(6)) as usize) << 7)
    };
    let raw_len = unsafe { read_volatile(payload.add(7)) as usize };
    if raw_len == 0 || raw_len > MAX_CHUNK_LEN {
        return;
    }

    let encoded_len = match raw_len.checked_mul(2) {
        Some(value) => value,
        None => return,
    };
    let checksum_index = 8 + encoded_len;
    let expected_message_len = checksum_index + 2; // checksum + F7
    if message_len != expected_message_len {
        return;
    }

    let end = match offset.checked_add(raw_len) {
        Some(value) => value,
        None => return,
    };
    if end > FRAMEBUFFER_LEN {
        return;
    }

    if unsafe { read_volatile(payload.add(checksum_index + 1)) } != 0xF7 {
        return;
    }

    let mut checksum = 0u8;
    let mut index = 0usize;
    while index < checksum_index {
        checksum ^= unsafe { read_volatile(payload.add(index)) };
        index += 1;
    }
    let expected_checksum = unsafe { read_volatile(payload.add(checksum_index)) };
    if checksum & 0x7F != expected_checksum {
        return;
    }

    let mut raw_index = 0usize;
    while raw_index < raw_len {
        let low = unsafe { read_volatile(payload.add(8 + raw_index * 2)) };
        let high = unsafe { read_volatile(payload.add(9 + raw_index * 2)) };
        if low > 0x0F || high > 0x0F {
            return;
        }
        let value = low | (high << 4);
        unsafe { write_volatile((FRAMEBUFFER + offset + raw_index) as *mut u8, value) };
        raw_index += 1;
    }
}

unsafe fn handle_present(payload: *mut u8, message_len: usize) {
    if message_len != 9 || unsafe { read_volatile(payload.add(8)) } != 0xF7 {
        return;
    }

    let expected_crc = unsafe {
        read_volatile(payload.add(5)) as u16
            | ((read_volatile(payload.add(6)) as u16) << 7)
            | ((read_volatile(payload.add(7)) as u16) << 14)
    };
    if expected_crc != unsafe { framebuffer_crc16() } {
        return;
    }

    type LcdFlush = unsafe extern "C" fn(*mut u8, *const u8) -> u32;
    let flush: LcdFlush = unsafe { core::mem::transmute(LCD_FLUSH_THUMB) };
    let _ = unsafe { flush(LCD_BUS as *mut u8, FRAMEBUFFER as *const u8) };
}

unsafe fn framebuffer_crc16() -> u16 {
    let mut crc = 0xFFFFu16;
    let mut index = 0usize;
    while index < FRAMEBUFFER_LEN {
        crc ^= (unsafe { read_volatile((FRAMEBUFFER + index) as *const u8) } as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
            bit += 1;
        }
        index += 1;
    }
    crc
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
