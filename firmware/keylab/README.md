# artupy firmware Rust scaffold

This crate is an **offline linker experiment**, not installable firmware.

It targets `thumbv7em-none-eabihf` (ARM Cortex-M4F) and models the memory ranges
inferred from Arturia firmware 1.2.1:

- application flash: `0x08007800`, 226 KiB;
- SRAM: `0x20000000`, 144 KiB;
- initial stack pointer: `0x20024000`.

The program contains only a minimal vector table and an infinite loop. It never
touches a peripheral. The project deliberately contains:

- no Arturia package/header generator;
- no USB or MIDI device access;
- no DFU implementation;
- no flash erase/write routine;
- no bootloader command.

Build it on the PC with:

```powershell
rustup target add thumbv7em-none-eabihf
cargo build --release
```

The resulting ELF is useful for checking architecture and linker placement.
Do not attempt to send it to the keyboard.
